use crate::docker::config::{DOCKER_IMAGE, DOCKERFILE};
use crate::docker::postgres::*;
use crate::misc::*;
use anyhow::Context;
use sqlparser::{
    ast::{Ident, Statement, UserDefinedTypeRepresentation},
    dialect::PostgreSqlDialect,
    parser::Parser,
};
use std::{
    fs,
    path::Path,
    process::{Command, Stdio},
};

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
pub(crate) fn rebuild_tools_image() -> anyhow::Result<()> {
    println!("Rebuilding Diesel tools image...");
    // Remove the existing image (ignore failure)
    let _ = Command::new("docker")
        .args(["rmi", "-f", DOCKER_IMAGE])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    // Rebuild
    build_tools_image(&root_directory()?)
}

/// This function checks whether a local Docker image named `TOOLS_IMAGE`
/// already exists. If it does, nothing happens. Otherwise, it builds the
/// image using the `Dockerfile` found in `xtask/`.
///
/// ## Documentation
/// See the official Docker CLI docs for more details:
/// * https://docs.docker.com/engine/reference/commandline/build/
/// * https://docs.docker.com/reference/cli/docker/buildx/build/
///
pub(crate) fn build_tools_image(root: &Path) -> anyhow::Result<()> {
    if docker_image_exists(DOCKER_IMAGE) {
        println!("Diesel tools image already exists (use `cargo run -p xtask -- rebuild` to refresh).");
        return Ok(());
    }
    which::which("docker").context("Could not find `docker` on PATH.")?;
    // ----------------------------------------------
    run(
        Command::new("docker").current_dir(root).args([
            "build",
            // Names the image so it can be reused.
            "--tag",
            DOCKER_IMAGE,
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
fn run_diesel_print_schema(config: &PostgresConfig) -> anyhow::Result<()> {
    run_tools_shell(
        config,
        &format!("diesel print-schema > {TABLE_RS}"),
        "run diesel print-schema",
    )
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

fn enum_variant_name(value: &str) -> String {
    snake_to_pascal_case(value)
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
fn write_enum_mappings(config: &PostgresConfig) -> anyhow::Result<Vec<PgEnum>> {
    let sql: String = read_init_sql(&config.root)?;
    let enums: Vec<PgEnum> = parse_pg_enums(&sql)?;
    let mut output: String = String::from("use diesel_derive_enum::DbEnum;\n\n");
    for pg_enum in &enums {
        output.push_str(&render_pg_enum(pg_enum));
    }
    fs::write(config.root.join(ENUMS_RS), output).context("write PostgreSQL enum mappings")?;
    Ok(enums)
}

/// Extracts PostgreSQL enum declarations from `init.sql`.
///
/// This uses `sqlparser` rather than manual string scanning, so formatting,
/// whitespace, comments, and multi-line declarations are handled more reliably.
///
/// Returns a [`Vec`] of parsed [`PgEnum`] structs using
/// [`pg_enum_from_statement`] internally.
fn parse_pg_enums(sql: &str) -> anyhow::Result<Vec<PgEnum>> {
    let statements: Vec<Statement> = Parser::parse_sql(&PostgreSqlDialect {}, sql).context("parse init.sql")?;
    Ok(statements.into_iter().filter_map(pg_enum_from_statement).collect())
}

/// Converts one parsed SQL statement into [`PgEnum`] metadata
/// if it is a `CREATE TYPE ... AS ENUM` statement. Other statements
/// such as `CREATE TABLE` are ignored. The rust names will be the names
/// used in [`ENUMS_RS`].
fn pg_enum_from_statement(statement: Statement) -> Option<PgEnum> {
    let Statement::CreateType {
        name,
        representation: Some(UserDefinedTypeRepresentation::Enum { labels }),
        ..
    } = statement
    else {
        return None;
    };
    let sql_name: String = name.to_string();
    let enum_values: Vec<String> = labels
        .into_iter()
        .map(|value: Ident| value.to_string().trim_matches('\'').to_owned())
        .collect::<Vec<_>>();
    Some(PgEnum {
        rust_name: snake_to_pascal_case(&sql_name),
        style: infer_enum_casing(&enum_values),
        sql_name: sql_name,
        values: enum_values,
    })
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
        .map(|value: &String| format!("    {},\n", enum_variant_name(value)))
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
        .map(|e: &PgEnum| format!("--map '{} crate::db::enums::{}'", e.rust_name, e.rust_name))
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
fn run_diesel_ext_models(config: &PostgresConfig, enum_maps: &str) -> anyhow::Result<()> {
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
pub(crate) fn run_diesel_ext_insertables(config: &PostgresConfig, enum_maps: &str) -> anyhow::Result<()> {
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

pub(crate) fn write_combined_db_file(config: &PostgresConfig) -> anyhow::Result<()> {
    let table: String = fs::read_to_string(config.root.join(TABLE_RS)).context("read generated Diesel schema")?;

    let enums: String = fs::read_to_string(config.root.join(ENUMS_RS)).context("read generated enum mappings")?;

    let models: String = fs::read_to_string(config.root.join(MODELS_RS)).context("read generated Diesel models")?;

    let insertables: String =
        fs::read_to_string(config.root.join(INSERTABLES_RS)).context("read generated Diesel insertables")?;

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

    fs::write(config.root.join(DB_RS), output).context("write combined db.rs")
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
pub(crate) fn generate_schema() -> anyhow::Result<()> {
    let config = PostgresConfig::load()?;

    // Avoid race conditions -- immediately removing new container because timestamp is old
    refresh_idle_timer(&config.root)?;

    println!("Building Diesel tools image...");
    build_tools_image(&config.root)?;

    println!("Ensuring Docker network...");
    ensure_docker_network()?;

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
    let pg_enums: Vec<PgEnum> = write_enum_mappings(&config)?;
    let enum_maps: String = diesel_ext_enum_maps(&pg_enums);

    println!("Generating Queryable models...");
    run_diesel_ext_models(&config, &enum_maps)?;

    println!("Generating Insertable structs...");
    run_diesel_ext_insertables(&config, &enum_maps)?;

    println!("Removing unused lifetimes in insertables");
    remove_unused_lifetimes_from_structs(INSERTABLES_RS)?;

    write_combined_db_file(&config)?;
    refresh_idle_timer(&config.root)?;

    println!("Generated Diesel schema, models, insertables, and enum mappings from init.sql");
    Ok(())
}
