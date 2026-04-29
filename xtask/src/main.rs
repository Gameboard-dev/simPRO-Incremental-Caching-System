//! Uses an `xtask` invocation to generate Diesel insertables and queryables
//! from a database SQL definition file (`init.sql`)
#![allow(unused_lifetimes)]
use anyhow::{Context, bail};
use sqlparser::{
    ast::{
        Ident, Statement, UserDefinedTypeRepresentation,
    },
    dialect::PostgreSqlDialect,
    parser::Parser,
};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Output, Stdio},
    str::Chars,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

// -----------------------------------------------------------------------------
// Input
// -----------------------------------------------------------------------------

const INIT_SQL: &str = "init.sql";

// -----------------------------------------------------------------------------
// Docker
// -----------------------------------------------------------------------------

/// PostgreSQL image used.
const POSTGRES_IMAGE: &str = "postgres:16-alpine";

/// Name of the PostgreSQL container.
const POSTGRES_CONTAINER: &str = "simpro-schema-db";

/// Network used to allow containers to communicate.
const DOCKER_NETWORK: &str = "simpro-schema-net";

/// Name of the Docker image containing Diesel and related tooling.
const TOOLS_IMAGE: &str = "simpro-schema-tools";

/// Path to the Dockerfile used to build `TOOLS_IMAGE`.
const DOCKERFILE: &str = "xtask/Dockerfile";

// -----------------------------------------------------------------------------
// Cleanup
// -----------------------------------------------------------------------------

/// How long the container can remain unused before being removed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// File storing the PID of the background cleanup process.
const IDLE_PID_FILE: &str =
    "target/xtask/schema-db-idle.pid";

/// File storing the last time the database was used (UNIX timestamp).
const IDLE_TIMESTAMP_FILE: &str =
    "target/xtask/schema-db-last-used";

// -----------------------------------------------------------------------------
// Output
// -----------------------------------------------------------------------------

/// Generated consolidated `db` module.
const DB_RS: &str = "src/db.rs";

/// Generated table schema (via `diesel print-schema`).
const TABLE_RS: &str = "target/xtask/table.rs";

/// Generated PostgreSQL enum mappings.
const ENUMS_RS: &str = "target/xtask/enums.rs";

/// Generated read models (`diesel-ext`)
/// - https://github.com/abbychau/diesel_cli_ext/blob/master/README.md#to-generate-model-structs
const MODELS_RS: &str = "target/xtask/models.rs";

#[allow(unused)]
/// Generated write models (diesel-ext)
/// - https://github.com/abbychau/diesel_cli_ext/blob/master/README.md#to-generate-insertable-structs
const INSERTABLES_RS: &str = "target/xtask/insertables.rs";

// -----------------------------------------------------------------------------

/// Runs a command handler method invoked using:
/// ```ignore
/// cargo run -p xtask -- <COMMAND>
/// ```
fn main() -> anyhow::Result<()> {
    match env::args().nth(1).as_deref() {
        Some("generate" | "generate-schema") => {
            generate_schema()
        }
        Some("rebuild") => rebuild_tools_image(),
        Some("cleanup-schema-db") => cleanup_if_idle(),
        Some("help" | "-h" | "--help") | None => {
            print_help();
            Ok(())
        }
        Some(command) => {
            bail!("unknown xtask command: {command}")
        }
    }
}

/// Forces a rebuild of the Docker tools image with:
///
/// ```ignore
/// cargo run -p xtask -- rebuild
/// ```
///
/// This removes the existing `TOOLS_IMAGE` (if it exists)
/// and rebuilds it from the Dockerfile.
///
/// ```ignore
/// docker rmi -f <TOOLS_IMAGE>
/// docker build --tag <TOOLS_IMAGE> --file <DOCKERFILE> --pull .
/// ```
///
fn rebuild_tools_image() -> anyhow::Result<()> {
    println!("Rebuilding Diesel tools image...");
    // Remove the existing image (ignore failure)
    let _ = Command::new("docker")
        .args(["rmi", "-f", TOOLS_IMAGE])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Rebuild
    build_tools_image(&project_root()?)
}

#[derive(Debug)]
struct PgEnum {
    #[allow(unused)]
    /// SQL enum variant name (e.g. job_type)
    sql_name: String,
    /// Rust enum variant name (e.g. JobType)
    rust_name: String,
    /// PostgreSQL enum values, exactly as written in SQL.
    values: Vec<String>,
    /// Naming style used by `#[DbValueStyle = "..."]` in `diesel-derive-enum`.
    style: DbValueStyle,
}

/// Naming style used by `#[DbValueStyle = "..."]` in `diesel-derive-enum`.
#[derive(Debug, Clone, Copy)]
enum DbValueStyle {
    PascalCase,
    SnakeCase,
}

impl DbValueStyle {
    /// String expected by `diesel-derive-enum`.
    fn as_string(self) -> &'static str {
        match self {
            Self::PascalCase => "PascalCase",
            Self::SnakeCase => "snake_case",
        }
    }
}

/// Infers the naming style of PostgreSQL enum values.
/// * Mixed styles (e.g. `"Foo_bar"`) will be classified as `SnakeCase`.
fn infer_enum_casing(values: &[String]) -> DbValueStyle {
    if values.iter().all(|v| is_pascal_case(v)) {
        DbValueStyle::PascalCase
    } else {
        DbValueStyle::SnakeCase
    }
}

/// Diesel generates SQL marker types for PostgreSQL enums in `table.rs`, but
/// `diesel_ext` does not automatically know which Rust enum type should be used
/// for those columns. This function bridges that gap by generating Rust enums
/// implementing the `DbEnum` derive from the `diesel-derive-enum` crate.
///
/// The `init.sql` file is parsed using the `sqlparser` crate with parsing code in
/// [`parse_pg_enums`] and returns a [`Vec`] of [`PgEnum`] used in [`diesel_ext_enum_maps`]
/// in the second stage in [`generate_schema`].
fn write_enum_mappings(
    config: &Config,
) -> anyhow::Result<Vec<PgEnum>> {
    let sql: String = read_init_sql(&config.root)?;
    let enums: Vec<PgEnum> = parse_pg_enums(&sql)?;
    let mut output: String =
        String::from("use diesel_derive_enum::DbEnum;\n\n");
    for pg_enum in &enums {
        output.push_str(&render_pg_enum(pg_enum));
    }
    fs::write(config.root.join(ENUMS_RS), output)
        .context("write PostgreSQL enum mappings")?;
    Ok(enums)
}

/// Extracts PostgreSQL enum declarations from `init.sql`.
///
/// This uses `sqlparser` rather than manual string scanning, so formatting,
/// whitespace, comments, and multi-line declarations are handled more reliably.
///
/// Returns a [`Vec`] of parsed [`PgEnum`] structs using
/// [`pg_enum_from_statement`] internally.
fn parse_pg_enums(
    sql: &str,
) -> anyhow::Result<Vec<PgEnum>> {
    let statements: Vec<Statement> =
        Parser::parse_sql(&PostgreSqlDialect {}, sql)
            .context("parse init.sql")?;
    Ok(statements
        .into_iter()
        .filter_map(pg_enum_from_statement)
        .collect())
}

/// Converts one parsed SQL statement into [`PgEnum`] metadata
/// if it is a `CREATE TYPE ... AS ENUM` statement. Other statements
/// such as `CREATE TABLE` are ignored. The rust names will be the names
/// used in [`ENUMS_RS`].
fn pg_enum_from_statement(
    statement: Statement,
) -> Option<PgEnum> {
    let Statement::CreateType {
        name,
        representation:
            Some(UserDefinedTypeRepresentation::Enum { labels }),
        ..
    } = statement
    else {
        return None;
    };
    let sql_name: String = name.to_string();
    let enum_values: Vec<String> = labels
        .into_iter()
        .map(|value: Ident| {
            value.to_string().trim_matches('\'').to_owned()
        })
        .collect::<Vec<_>>();
    Some(PgEnum {
        rust_name: snake_to_pascal_case(&sql_name),
        style: infer_enum_casing(&enum_values),
        sql_name: sql_name,
        values: enum_values,
    })
}

/// Reads the `init.sql` file from the project root.
/// * Returns an error if the file cannot be read.
fn read_init_sql(root: &Path) -> anyhow::Result<String> {
    fs::read_to_string(root.join(INIT_SQL))
        .context("read init.sql")
}

/// Renders a [`PgEnum`] into Rust source code with the `DbEnum` derive
/// from `diesel-derive-enum` and the enum type path from the generated schema file
///
/// This produces a complete enum definition with inferred `DbValueStyle` (e.g. `PascalCase`),
/// for example:
///
/// ```ignore
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]
/// #[ExistingTypePath = "crate::db::table::sql_types::JobType"]
/// #[DbValueStyle = "PascalCase"]
/// pub enum JobType {
///     Project,
///     Service,
///     Prepaid,
/// }
/// ```
fn render_pg_enum(pg_enum: &PgEnum) -> String {
    let variants: String = pg_enum
        .values
        .iter()
        .map(|value: &String| {
            format!("    {},\n", enum_variant_name(value))
        })
        .collect::<String>();
    format!(
        "#[derive(Debug, Clone, Copy, PartialEq, Eq, DbEnum)]\n\
         #[ExistingTypePath = \"crate::db::table::sql_types::{rust_name}\"]\n\
         #[DbValueStyle = \"{style}\"]\n\
         pub enum {rust_name} {{\n\
         {variants}\
         }}\n\n",
        rust_name = pg_enum.rust_name,
        style = pg_enum.style.as_string(),
    )
}

/// The `diesel_ext_cli` command client does not automatically
/// understand custom PostgreSQL enums.
///
/// Instead, each enum must be explicitly mapped
/// using the `--map` flag.
///
/// This function builds the `--map` arguments required by `diesel_ext`
/// to associate PostgreSQL enum types with their corresponding Rust enum types.
///
/// ## Example
/// ```ignore,bash
/// --map 'JobType crate::db::enums::JobType' --map 'ScheduleType crate::db::enums::ScheduleType'
/// ```
///
/// ## Documentation
/// * https://github.com/abbychau/diesel_cli_ext/blob/master/docs/index.md
/// * https://stackoverflow.com/a/79594231
///
fn diesel_ext_enum_maps(enums: &[PgEnum]) -> String {
    enums
        .iter()
        .map(|e: &PgEnum| {
            format!(
                "--map '{} crate::db::enums::{}'",
                e.rust_name, e.rust_name
            )
        })
        .collect::<Vec<_>>() // required for join()
        .join(" ")
}

/// Generate Diesel-compatible models from the schema using diesel_ext.
///
/// This reads the schema defined in `TABLE_RS` (generated by Diesel)
/// and outputs Rust structs into `MODELS_RS` with the specified derives.
///
/// This implements both `Insertable` and `AsChangeset` since
/// `--insertable` generates structs with no `id` field assuming `id` is skippable.
///
/// ```ignore
/// #[derive(Debug, Clone, Queryable, Selectable)]
/// pub struct User {
///     pub id: i32,
///     pub name: String,
/// }
/// ```
///
/// # Errors
/// Returns an error if the CLI tool fails or the schema file cannot be processed.
///
/// # Documentation
/// * https://github.com/abbychau/diesel_cli_ext/blob/master/README.md
///
fn run_diesel_ext_models(
    config: &Config,
    enum_maps: &str,
) -> anyhow::Result<()> {
    let imports: &str = "crate::db::table::*";
    run_tools_shell(
        config,
        &format!(
            "diesel_ext \
             --schema-file {TABLE_RS} \
             --model \
             --add-table-name \
             --import-types '{imports}' \
             {enum_maps} \
             --derive 'Debug, Clone, Queryable, Selectable' \
             > {MODELS_RS}"
        ),
        "generate Diesel models",
    )
}

#[allow(unused)]
/// This follows Diesel's convention of using separate structs for query and insertion
/// where columns allow for database-generated or optional values.
/// * Automatically skips common auto-generated fields (id, created_at, updated_at)
/// * Supports custom field skipping and optional field configuration
///
/// ```ignore,rust
/// #[derive(Debug, Clone, Insertable, AsChangeset)]
/// #[diesel(table_name = users)]
/// pub struct NewUser {
///     pub id: i32,
///     pub name: String,
/// }
///
/// ```
///
/// # Errors
/// This will return an error if the CLI tool fails or the schema file cannot be processed.
///
/// # Documentation
/// * https://github.com/abbychau/diesel_cli_ext/blob/master/README.md#to-generate-insertable-structs
///
fn run_diesel_ext_insertables(
    config: &Config,
    enum_maps: &str,
) -> anyhow::Result<()> {
    // --import-types can be specified more than once in the arguments for multiple imports
    let imports: &str = "crate::db::table::*";
    run_tools_shell(
        config,
        &format!(
            "diesel_ext \
             --schema-file {TABLE_RS} \
             --insertable \
             --add-table-name \
             --import-types '{imports}' \
             --skip-fields 'idx' \
             {enum_maps} \
             --derive 'Debug, Clone, Insertable, AsChangeset' \
             > {INSERTABLES_RS}"
        ),
        "generate Diesel Insertable structs",
    )
}

fn write_combined_db_file(
    config: &Config,
) -> anyhow::Result<()> {
    let table: String =
        fs::read_to_string(config.root.join(TABLE_RS))
            .context("read generated Diesel schema")?;

    let enums: String =
        fs::read_to_string(config.root.join(ENUMS_RS))
            .context("read generated enum mappings")?;

    let models: String =
        fs::read_to_string(config.root.join(MODELS_RS))
            .context("read generated Diesel models")?;

    let insertables: String = fs::read_to_string(
        config.root.join(INSERTABLES_RS),
    )
    .context("read generated Diesel insertables")?;

    let output = format!(
        "// @generated by xtask. Do not edit by hand.\n\n\
         pub mod table {{\n\
         {table}\n\
         }}\n\n\
         pub mod enums {{\n\
         {enums}\n\
         }}\n\n\
         pub mod models {{\n\
         // Read-only models\n\
         {models}\n\
         }}\n\n\
         pub mod insertables {{\n\
         // Write-only models\n\
         {insertables}\n\
         }}\n"
    );

    fs::write(config.root.join(DB_RS), output)
        .context("write combined db.rs")
}

/// Runs a shell command inside an ethereal container with `diesel` and
/// `diesel_ext` dependencies.
///
/// The project root is mounted into the container at `/work`, and the command
/// is executed from that directory.
///
/// `DATABASE_URL` is passed into the container so Diesel can connect to the
/// temporary PostgreSQL container over the shared Docker network.
///
/// This runs the equivalent of:
/// ```ignore
/// docker run --rm \
///   --network <DOCKER_NETWORK> \
///   --volume <PROJECT_ROOT>:/work \
///   --workdir /work \
///   --env DATABASE_URL=<URL> \
///   <TOOLS_IMAGE> \
///   sh --command "<script>"
/// ```
///
/// ## Documentation
/// - Docker `run`: https://docs.docker.com/engine/reference/commandline/run/
/// - Docker volumes: https://docs.docker.com/engine/storage/volumes/
///
/// ## Returns
/// * Returns an error if Docker fails to start the container or the command inside
///   the container exits unsuccessfully.
fn run_tools_shell(
    config: &Config,
    script: &str,
    action: &str,
) -> anyhow::Result<()> {
    run(
        Command::new("docker")
            .current_dir(&config.root)
            .args([
                "run",
                // `--rm` removes the tools container after the command finishes.
                "--rm",
                // `--network` attaches it to the same Docker network as PostgreSQL.
                "--network",
                DOCKER_NETWORK,
                // `--volume` mounts (maps) the project directory where generated files are written back.
                "--volume",
                &format!("{}:/work", config.root.display()),
                // `--workdir` makes `/work` the container's current directory.
                "--workdir",
                "/work",
                // `--env DATABASE_URL=...` provides the database connection string to Diesel.
                "--env",
                &format!(
                    "DATABASE_URL={}",
                    config.database_url()
                ),
                // The fixed name of the image the container will use ("simpro-schema-tools")
                TOOLS_IMAGE,
                // `sh -c "<script>"` runs the supplied shell command
                "sh",
                "-c",
                script,
            ]),
        action,
    )
}

fn print_help() {
    println!("Commands:");
    println!("  cargo xtask generate");
    println!("  cargo xtask generate-schema");
}

/// Regenerates Diesel files from `init.sql`.
///
/// The temporary PostgreSQL container is reused briefly between runs to avoid
/// paying container startup cost repeatedly, but its schema is reset before each
/// generation so the output remains deterministic.
///
/// 1. diesel print-schema     → generates table.rs, including sql_types::JobType
/// 2. generate enums.rs       → defines Rust enum wrappers using diesel-derive-enum
/// 3. diesel_ext --map ...    → generates models/insertables using tables/enums
fn generate_schema() -> anyhow::Result<()> {
    let config = Config::load()?;

    // Avoid race conditions -- immediately removing new container because timestamp is old
    refresh_idle_timer(&config.root)?;

    println!("Building Diesel tools image...");
    build_tools_image(&config.root)?;

    println!("Ensuring Docker network...");
    ensure_network()?;

    println!("Ensuring Postgres container...");
    ensure_postgres(&config)?;

    println!("Waiting for Postgres...");
    wait_for_postgres(&config)?;

    println!("Resetting schema...");
    reset_schema(&config)?;

    println!("Applying init.sql...");
    apply_init_sql(&config)?;

    println!("Generating Diesel schema...");
    run_diesel_print_schema(&config)?;

    println!("Generating PostgreSQL enum mappings...");
    let pg_enums: Vec<PgEnum> =
        write_enum_mappings(&config)?;
    let enum_maps: String = diesel_ext_enum_maps(&pg_enums);

    println!("Generating Queryable models...");
    run_diesel_ext_models(&config, &enum_maps)?;

    println!("Generating Insertable structs...");
    run_diesel_ext_insertables(&config, &enum_maps)?;

    println!("Removing unused lifetimes in insertables");
    remove_unused_lifetimes_from_structs(INSERTABLES_RS)?;

    write_combined_db_file(&config)?;

    refresh_idle_timer(&config.root)?;

    println!(
        "Generated Diesel schema, models, insertables, and enum mappings from init.sql"
    );
    Ok(())
}

struct Config {
    root: PathBuf,
    db_user: String,
    db_password: String,
    db_name: String,
}

impl Config {
    fn load() -> anyhow::Result<Self> {
        let root: PathBuf = project_root()?;
        dotenvy::from_path(root.join(".env")).ok();
        Ok(Self {
            root,
            db_user: require_env("POSTGRES_USER")?,
            db_password: require_env("POSTGRES_PASSWORD")?,
            db_name: require_env("POSTGRES_DB")?,
        })
    }

    fn database_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:5432/{}",
            self.db_user,
            self.db_password,
            POSTGRES_CONTAINER,
            self.db_name
        )
    }
}

// -----------------------------------------------------------------------------
// Docker lifecycle
// -----------------------------------------------------------------------------

/// This function checks whether a local Docker image named `TOOLS_IMAGE`
/// already exists. If it does, nothing happens. Otherwise, it builds the
/// image using the `Dockerfile` found in `xtask/`.
///
/// ## Documentation
/// See the official Docker CLI docs for more details:
/// * https://docs.docker.com/engine/reference/commandline/build/
/// * https://docs.docker.com/reference/cli/docker/buildx/build/
///
fn build_tools_image(root: &Path) -> anyhow::Result<()> {
    if docker_image_exists(TOOLS_IMAGE) {
        println!(
            "Diesel tools image already exists (use `cargo run -p xtask -- rebuild` to refresh)."
        );
        return Ok(());
    }
    run(
        Command::new("docker").current_dir(root).args([
            "build",
            // Names the image so it can be reused.
            "--tag",
            TOOLS_IMAGE,
            // Specifies which Dockerfile to build from.
            "--file",
            DOCKERFILE,
            // Checks for newer versions of the base image.
            // This will only run if the container is rebuilt.
            "--pull",
            // Shows full build logs.
            "--progress=plain",
            // Sets the build context to the project directory.
            ".",
        ]),
        "build schema tools Docker image",
    )
}

/// This function checks whether a Docker network named `DOCKER_NETWORK`
/// exists. If it does, nothing happens. Otherwise, it creates it.
///
/// * A **Docker network** allows multiple containers to communicate with each other.
///   Containers on the same network can refer to each other by name (like a hostname).
/// *
/// * Without a Docker network, containers communicate via ports exposed on the host
///   machine (e.g. `localhost`). This is less straightforward and less reliable,
///   as networking behaves differently between Linux, WSL, and Windows/macOS.
///
/// This is called before starting any containers that need to communicate.
///
/// ## Errors
/// Returns an error if Docker fails to create the network.
///
/// ## Documentation
/// See the official Docker CLI docs:
/// https://docs.docker.com/engine/reference/commandline/network_create/
fn ensure_network() -> anyhow::Result<()> {
    if docker_network_exists(DOCKER_NETWORK) {
        return Ok(());
    }
    run(
        Command::new("docker").args([
            "network",
            "create",
            DOCKER_NETWORK,
        ]),
        "Create Docker Network",
    )
}

/// This ensures that a running PostgreSQL container is available for schema generation.
///
/// If it is, nothing happens. Otherwise, it removes any
/// existing **stopped** container with the same name
/// and starts a new one.
///
/// ## IMPORTANT
/// If .env changes while the container is already running,
/// the old Postgres container will still use the old user/password/database
/// until the container is reset after `IDLE_TIMEOUT`.
fn ensure_postgres(config: &Config) -> anyhow::Result<()> {
    if container_is_running(POSTGRES_CONTAINER) {
        return Ok(());
    }

    remove_container_if_exists(POSTGRES_CONTAINER);

    run(
        Command::new("docker").args([
            "run",
            // Runs the container in the background (detached mode).
            "-d",
            // Assigns a fixed name so other containers can refer to it.
            "--name",
            POSTGRES_CONTAINER,
            // Connects the container to the shared network for inter-container communication.
            "--network",
            DOCKER_NETWORK,
            // Sets environment variables used by PostgreSQL to initialise the database.
            "-e",
            &format!("POSTGRES_USER={}", config.db_user),
            "-e",
            &format!(
                "POSTGRES_PASSWORD={}",
                config.db_password
            ),
            "-e",
            &format!("POSTGRES_DB={}", config.db_name),
            // The PostgreSQL image used to run the database.
            // E.G. "postgres:16-alpine".
            POSTGRES_IMAGE,
        ]),
        "Start a reuable Postgres container",
    )
}

/// Waits for the PostgreSQL container to become ready to accept connections.
///
/// This polls `postgres_is_ready`, which runs `pg_isready`
/// inside the container via `docker exec`.
/// - Polls up to 60 times
/// - Waits 500ms between attempts
/// - Waits up to ~30 seconds
///
/// If PostgreSQL reports readiness (`pg_isready` succeeds), this returns `Ok(())`.
///
/// ## Errors
/// Returns an error if PostgreSQL does not become ready within the timeout.
///
/// ## Documentation
/// - https://www.postgresql.org/docs/current/app-pg-isready.html
///
fn wait_for_postgres(
    config: &Config,
) -> anyhow::Result<()> {
    for _ in 0..60 {
        if postgres_is_ready(config) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    bail!(
        "temporary Postgres container did not become ready"
    )
}

fn docker_network_exists(name: &str) -> bool {
    command_succeeds(
        Command::new("docker")
            .args(["network", "inspect", name]),
    )
}

fn container_is_running(name: &str) -> bool {
    output(
        Command::new("docker").args([
            "inspect",
            "-f",
            "{{.State.Running}}",
            name,
        ]),
        "inspect Docker container",
    )
    .is_ok_and(|stdout| stdout.trim() == "true")
}

/// # Postgres Is Ready
///
/// Check whether the PostgreSQL container is ready to accept connections.
///
/// This runs the `pg_isready` command inside the running PostgreSQL
/// container to determine whether the database is fully initialised and ready
/// for use. It's used in a polling loop in `wait_for_postgres`.
///
/// ## Documentation
/// * https://www.postgresql.org/docs/current/app-pg-isready.html
/// * https://docs.docker.com/engine/reference/commandline/exec/
///
fn postgres_is_ready(config: &Config) -> bool {
    command_succeeds(Command::new("docker").args([
        "exec",
        POSTGRES_CONTAINER,
        "pg_isready",
        "-U",
        &config.db_user,
        "-d",
        &config.db_name,
    ]))
}

/// Attempts to delete a container with the given name using
/// `docker rm -f`. If the container does not exist or removal fails, the
/// error is ignored.
///
/// ## Documentation
/// See the official Docker CLI docs:
/// https://docs.docker.com/engine/reference/commandline/rm/
///
fn remove_container_if_exists(name: &str) {
    let _ = Command::new("docker")
        // `rm` (remove)
        // `-f` (force)
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

// -----------------------------------------------------------------------------
// Schema generation
// -----------------------------------------------------------------------------

/// Drops the schema in the running PostgreSQL container by running:
///
/// ```sql
/// DROP SCHEMA IF EXISTS public CASCADE;
/// CREATE SCHEMA public;
/// ```
///
/// This drops and recreates the `public` schema in the temporary PostgreSQL
/// container before `init.sql` is applied again. This keeps schema generation
/// deterministic: each run starts from a clean database state.
///
/// This only runs if the container is still running when `init.sql` is saved.
/// If the container was cleaned up after `IDLE_TIMEOUT` a new container is started.
///
/// ## Errors
/// Returns an error if the temporary PostgreSQL container is not running,
/// `psql` fails, or the SQL command fails.
///
fn reset_schema(config: &Config) -> anyhow::Result<()> {
    run_psql(
        config,
        &[
            "-c",
            "DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;",
        ],
        "Rest temporary Postgres schema",
    )
}

fn apply_init_sql(config: &Config) -> anyhow::Result<()> {
    let init_sql = config.root.join(INIT_SQL);

    run(
        Command::new("docker").args([
            "cp",
            init_sql.to_str().context(
                "init.sql path is not valid UTF-8",
            )?,
            &format!("{POSTGRES_CONTAINER}:/init.sql"),
        ]),
        "copy init.sql into a temporary Postgres container",
    )?;

    run_psql(config, &["-f", "/init.sql"], "apply init.sql")
}

/// Generates a Diesel Rust schema file from a live PostgreSQL database.
///
/// ```bash
/// diesel print-schema > table.rs
/// ```
///
/// This should be called after the PostgreSQL container is running and
/// `init.sql` has been applied as it introspects the live database schema
/// using `DATABASE_URL`.
///
/// The generated file is used by Diesel to build type-safe queries
/// or by `diesel_ext` to generate model structs.
///
/// Returns an error if Diesel cannot connect to the database, the schema
/// cannot be introspected, or the command fails.
///
/// ## Configuration
/// Diesel reads the database connection string from:
/// - `DATABASE_URL`, e.g. `postgres://user:password@host:port/database`
///
/// ## Documentation
/// - https://diesel.rs/guides/getting-started/
/// - https://docs.diesel.rs/diesel_cli/command/print_schema/
///
fn run_diesel_print_schema(
    config: &Config,
) -> anyhow::Result<()> {
    run_tools_shell(
        config,
        &format!("diesel print-schema > {TABLE_RS}"),
        "run diesel print-schema",
    )
}

/// Before building or pulling an image, it can be useful to check whether
/// it already exists locally to avoid unnecessary work:
///
/// ```bash
/// docker image inspect <IMAGE_NAME> # (e.g. `rust:1-bookworm`)
/// ```
///
/// Any failure (e.g. image not found, Docker unavailable) results in `false`.
///
fn docker_image_exists(name: &str) -> bool {
    command_succeeds(
        Command::new("docker")
            .args(["image", "inspect", name]),
    )
}

/// Executes a `psql` command inside a running PostgreSQL container.
/// * This assumes PGSQL is running and ready to accept commands.
///
/// ```bash
/// docker exec <POSTGRES_CONTAINER> \
///   psql -U <user> -d <database> \
///   -v ON_ERROR_STOP=1 \
///   <args...>
/// ```
///
/// - `docker exec`  
///   Runs a command inside an already running container.
///
/// - `psql`  
///   The PostgreSQL command-line tool used to execute SQL.
///
/// - `-U <user>`  
///   The database user to connect as.
///
/// - `-d <database>`  
///   The database to connect to.
///
/// - `-v ON_ERROR_STOP=1`  
///   Instructs `psql` to stop immediately if any SQL statement fails.
///
/// - `<args...>`  
///   Arguments passed to `psql`, such as:
///   - `-f file.sql` → execute an SQL file  
///   - `-c "SQL"` → execute an SQL command
///
/// ## Return value
/// Returns an error if the command fails
/// (E.G. Container was not running, SQL was invalid).
///
/// ## Documentation
///
/// * PostgreSQL `psql`:
///   https://www.postgresql.org/docs/current/app-psql.html
///
/// * Docker `exec`:
///   https://docs.docker.com/engine/reference/commandline/exec/
///
fn run_psql(
    config: &Config,
    args: &[&str],
    action: &str,
) -> anyhow::Result<()> {
    let mut docker_args = vec![
        "exec",
        POSTGRES_CONTAINER,
        "psql",
        "-U",
        &config.db_user,
        "-d",
        &config.db_name,
        "-v",
        "ON_ERROR_STOP=1",
    ];

    docker_args.extend_from_slice(args);

    run(Command::new("docker").args(docker_args), action)
}

// -----------------------------------------------------------------------------
// Idle cleanup
// -----------------------------------------------------------------------------

/// Marks the temporary database as recently used and ensures one cleanup process is scheduled.
fn refresh_idle_timer(root: &Path) -> anyhow::Result<()> {
    write_timestamp(root.join(IDLE_TIMESTAMP_FILE))?;
    ensure_cleanup_process(root)
}

/// Ensures a single background cleanup process is running for the container service.
///
/// This starts a detached xtask process that will shut down the temporary PostgreSQL
/// container after a period of inactivity (`IDLE_TIMEOUT`) using:
///
/// ```ignore
/// xtask cleanup-schema-db
/// ```
///
/// This is handled by `cleanup_schema_db_after_delay()` which sleeps for
/// `IDLE_TIMEOUT` and calls:
///
/// ```ignore
/// remove_container_if_exists(POSTGRES_CONTAINER);
/// ```
///
/// Which calls:
///
/// ```ignore
/// docker rm -f <POSTGRES_CONTAINER>
/// ```
///
/// The PID of the cleanup process is stored in `IDLE_PID_FILE` so
/// repeated generation invocations do not
/// start multiple cleanup processes.
///
/// ### Errors
/// Returns an error if the cleanup process cannot be started or its PID cannot
/// be written to disk.
///
fn ensure_cleanup_process(
    root: &Path,
) -> anyhow::Result<()> {
    if cleanup_process_is_running(root) {
        return Ok(());
    }

    let pid_file = root.join(IDLE_PID_FILE);
    create_parent_dir(&pid_file)?;

    // Reuse the existing `xtask` executable.
    let mut command = Command::new(
        env::current_exe()
            .context("locate xtask executable")?,
    );

    let child = detach_command_io(
        command.arg("cleanup-schema-db").current_dir(root),
    )
    // Execute the command as as a non-blocking subprocess.
    .spawn()
    .context("begin schema DB cleanup process")?;

    // Save the PID file with contents `child.id()`.
    fs::write(pid_file, child.id().to_string())?;

    Ok(())
}

/// Returns `true` if the `IDLE_PID_FILE` exists
/// and the PID still belongs to a running process.
fn cleanup_process_is_running(root: &Path) -> bool {
    fs::read_to_string(root.join(IDLE_PID_FILE))
        .is_ok_and(|pid| process_is_running(pid.trim()))
}

/// Detaches a child process from the terminal by discarding its standard streams.
///
/// Every process has three standard I/O streams:
/// - **stdin**  (standard input)  → where the program reads input from  
/// - **stdout** (standard output) → where normal output is written  
/// - **stderr** (standard error)  → where error messages are written  
///
/// This function redirects all three to `Stdio::null()`, which is equivalent to
/// sending them to `/dev/null` (on Unix) or `NUL` (on Windows).
///
/// ## Documentation
/// - https://doc.rust-lang.org/std/process/struct.Stdio.html  
/// - https://en.wikipedia.org/wiki/Standard_streams
///
fn detach_command_io(
    command: &mut Command,
) -> &mut Command {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
}

/// Checks whether a process with the given PID (Process ID) is still running.
///
/// On Windows, this uses:
/// ```ignore
/// tasklist /FI "PID eq <pid>"
/// ```
///
/// On Unix, including Linux, macOS, and WSL, this uses:
/// ```ignore
/// kill -0 <pid>
/// ```
/// *  `kill -0` does **not** kill the process. It only asks the operating system
///    whether a process with that PID exists and can be signalled.
///
fn process_is_running(pid: &str) -> bool {
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    #[cfg(windows)]
    {
        Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}")])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .map(|output: Output| {
                output.status.success()
                    && String::from_utf8_lossy(
                        &output.stdout,
                    )
                    .contains(&pid.to_string())
            })
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        command_succeeds(
            Command::new("kill")
                .args(["-0", &pid.to_string()]),
        )
    }
}

/// Removes the temporary PostgreSQL container once it has been idle for
/// at least `IDLE_TIMEOUT`.
///
/// Invoked by `xtask cleanup-schema-db` and repeatedly performs the following:
/// 1. Sleep for `IDLE_TIMEOUT`
/// 2. Read the last-used timestamp from `IDLE_TIMESTAMP_FILE`
/// 3. Compare it with the current time (`now_unix_seconds()`)
///
/// In each iteration, the container is removed only if:
///
/// ```ignore
/// (current_time - last_used_time) >= IDLE_TIMEOUT
/// ```
///
/// The behaviour:
/// - If the container was used recently → the process continues looping
/// - If the container has been idle long enough → it is removed and the process exits
///
/// which ensures:
/// - Only one cleanup process remains active (via `IDLE_PID_FILE`)
/// - The container is reused during active development
/// - The container is eventually cleaned up after inactivity
/// - The `IDLE_PID_FILE` is removed at the same time.
///
/// This function will NOT run again if `cleanup_process_is_running()` returns `true`
/// on another `generate-schema` invocation (i.e., in `cargo watch`) so only one loop
/// should ever be running.
///
/// ## Edge cases
/// - If the timestamp file is missing or unreadable, cleanup proceeds
/// - This is safe because the container is ephemeral and only used for schema generation
///
fn cleanup_if_idle() -> anyhow::Result<()> {
    let root: PathBuf = project_root()?;

    loop {
        thread::sleep(IDLE_TIMEOUT);

        let should_cleanup =
            read_timestamp(root.join(IDLE_TIMESTAMP_FILE))?
                .map(|last_used| {
                    now_unix_seconds()
                        .saturating_sub(last_used)
                        >= IDLE_TIMEOUT.as_secs()
                })
                .unwrap_or(true);

        if should_cleanup {
            remove_container_if_exists(POSTGRES_CONTAINER);
            remove_file_if_exists(root.join(IDLE_PID_FILE));
            return Ok(());
        }
    }
}

// -----------------------------------------------------------------------------
// Utilities
// -----------------------------------------------------------------------------

/// `CARGO_MANIFEST_DIR` is set at compile time and points to the directory
/// containing this crate’s `Cargo.toml` (i.e. the `xtask/` directory).
///
/// This returns its parent (..), which is assumed to be the
/// workspace or project root directory.
fn project_root() -> anyhow::Result<PathBuf> {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask directory has no parent")
        .map(Path::to_path_buf)
}

fn require_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| {
        format!(
            "{name} is missing from .env or the environment"
        )
    })
}

/// Runs the provided [`Command`] and returns `true` if it exits with a
/// successful status code (`0`), or `false` if the command fails to start,
/// exits with a non-zero status, or produces an error.
///
/// Errors are converted into `false` and `stdout` and `stderr` are
/// discarded (to `/dev/null`).
///
/// This is used for *existence checks* and *probes* where failure is expected
/// and not exceptional.
///
/// If you need error details, use a function like `run` or `output` instead.
fn command_succeeds(command: &mut Command) -> bool {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn write_timestamp(path: PathBuf) -> anyhow::Result<()> {
    create_parent_dir(&path)?;
    fs::write(path, now_unix_seconds().to_string())?;
    Ok(())
}

/// Reads a timestamp (as `u64`) from a file
fn read_timestamp(
    path: PathBuf,
) -> anyhow::Result<Option<u64>> {
    fs::read_to_string(path)
        .ok()
        .map(|s| {
            s.trim()
                .parse::<u64>()
                .context("idle timestamp file is invalid")
        })
        .transpose()
}

fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn remove_file_if_exists(path: PathBuf) {
    let _ = fs::remove_file(path);
}

fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is before UNIX epoch")
        .as_secs()
}

/// Executes a command and checks only for success or failure.
///
/// The `action` string is used to provide contextual error messages and
/// logging output, making failures easier to understand.
///
/// # Example
/// ```ignore
/// run(Command::new("docker").args(["build", "."]), "build image")?;
/// ```
///
/// Use if:
/// - You only care about success/failure
/// - You want real-time output in the terminal (i.e., long-running commands)
///
/// Use [`output`] instead if you need to make decisions
/// based on the command output.
///
/// # Errors
/// Returns an error if:
/// - The command cannot be started
/// - The command exits with a non-zero status
fn run(
    command: &mut Command,
    action: &str,
) -> anyhow::Result<()> {
    println!("→ {action}");

    let status: ExitStatus = command
        .status()
        .with_context(|| format!("failed to {action}"))?;

    if !status.success() {
        bail!("failed to {action} with status {status}");
    }

    Ok(())
}

/// Executes a command and captures its standard output (`stdout`)
/// and errors (`stderr`).
///
/// The `action` string is used to provide contextual error messages.
///
/// # Example
/// ```ignore
/// let output = output(
///     Command::new("docker").args(["inspect", "my-container"]),
///     "inspect container"
/// )?;
/// ```
///
/// Use if:
/// - You need to read or parse the command output
/// - You need to make decisions based on stdout
///
/// Use [`run`] instead if you only care about success/failure
/// and want streaming output.
///
/// # Errors
/// - If the command cannot be started
/// - If the command exits with a non-zero status
/// - If `stdout` is not valid UTF-8
fn output(
    command: &mut Command,
    action: &str,
) -> anyhow::Result<String> {
    let output: Output = command
        .output()
        .with_context(|| format!("failed to {action}"))?;

    if !output.status.success() {
        bail!(
            "Failed to {action}\n\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    String::from_utf8(output.stdout)
        .context("command stdout was not valid UTF-8")
}

fn snake_to_pascal_case(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(capitalize)
        .collect()
}

fn enum_variant_name(value: &str) -> String {
    snake_to_pascal_case(value)
}

/// Converts the first character of `value` to uppercase and leaves the
/// remaining characters unchanged.
fn capitalize(value: &str) -> String {
    let mut chars: Chars<'_> = value.chars();
    match chars.next() {
        Some(first) => {
            first.to_uppercase().collect::<String>()
                + chars.as_str()
        }
        None => String::new(),
    }
}

/// Classifies a `&str` as PascalCase if its first char is
/// uppercase ASCII and it contains no underscores (`_`).
fn is_pascal_case(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_uppercase())
        && !value.contains('_')
}

/// A small function to cope with the temporary inconvenience of
/// `diesel_cli_ext --insertables` generates structs with unused lifetimes
/// which triggers a compiler warning
fn remove_unused_lifetimes_from_structs(
    path: &str,
) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::collections::BTreeSet;
    use syn::visit::Visit;
    use syn::visit_mut::{self, VisitMut};
    struct RemoveUnusedLifetimes;

    impl VisitMut for RemoveUnusedLifetimes {
        fn visit_item_struct_mut(
            &mut self,
            item: &mut syn::ItemStruct,
        ) {
            visit_mut::visit_item_struct_mut(self, item);
            let used_lifetimes: BTreeSet<String> =
                lifetimes_used_in_fields(&item.fields);

            item.generics.params = item
                .generics
                .params
                .clone()
                .into_iter()
                .filter(|param: &syn::GenericParam| {
                    match param {
                        syn::GenericParam::Lifetime(
                            lifetime,
                        ) => used_lifetimes.contains(
                            &lifetime
                                .lifetime
                                .ident
                                .to_string(),
                        ),
                        _ => true,
                    }
                })
                .collect();
        }
    }

    fn lifetimes_used_in_fields(
        fields: &syn::Fields,
    ) -> BTreeSet<String> {
        struct LifetimeCollector {
            lifetimes: BTreeSet<String>,
        }
        impl<'ast> Visit<'ast> for LifetimeCollector {
            fn visit_lifetime(
                &mut self,
                lifetime: &'ast syn::Lifetime,
            ) {
                self.lifetimes
                    .insert(lifetime.ident.to_string());
            }
        }
        let mut collector: LifetimeCollector =
            LifetimeCollector {
                lifetimes: Default::default(),
            };
        collector.visit_fields(fields);
        collector.lifetimes
    }

    let source: String = std::fs::read_to_string(path)
        .with_context(|| {
            format!("read generated Rust file {path}")
        })?;

    let mut file: syn::File = syn::parse_file(&source)
        .with_context(|| {
            format!("parse generated Rust file {path}")
        })?;

    RemoveUnusedLifetimes.visit_file_mut(&mut file);

    std::fs::write(path, prettyplease::unparse(&file))
        .with_context(|| {
            format!("write postprocessed Rust file {path}")
        })?;

    Ok(())
}
