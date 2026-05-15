//! Manages the PostgreSQL container system
//! and cleanup after an idle period
//! using a timestamp file with its PID

use crate::INIT_SQL;
use crate::docker::config::{DOCKER_IMAGE, DOCKER_NETWORK};
use crate::misc::*;
use anyhow::{Context, bail};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    thread,
    time::Duration,
};

/// PostgreSQL image used.
pub(crate) const POSTGRES_IMAGE: &str = "postgres:18";

/// Name of the PostgreSQL container.
pub(crate) const POSTGRES_CONTAINER: &str = "simpro-schema-db";

/// How long the container can remain unused before being removed.
const IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// File storing the PID of the background cleanup process.
const IDLE_PID_FILE: &str = "target/xtask/schema-db-idle.pid";

/// File storing the last time the database was used (UNIX timestamp).
const IDLE_TIMESTAMP_FILE: &str = "target/xtask/schema-db-last-used";

pub(crate) struct PostgresConfig {
    pub(crate) root: PathBuf,
    pub(crate) db_user: String,
    pub(crate) db_password: String,
    pub(crate) db_name: String,
}

impl PostgresConfig {
    pub(crate) fn load() -> anyhow::Result<Self> {
        let root: PathBuf = root_directory()?;
        dotenvy::from_path(root.join(".env")).ok();
        Ok(Self {
            root,
            db_user: require_env("POSTGRES_USER")?,
            db_password: require_env("POSTGRES_PASSWORD")?,
            db_name: require_env("DATABASE_NAME")?,
        })
    }

    pub(crate) fn database_url(&self) -> String {
        format!(
            "postgres://{}:{}@{}:5432/{}",
            self.db_user, self.db_password, POSTGRES_CONTAINER, self.db_name
        )
    }
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
pub(crate) fn ensure_postgres(config: &PostgresConfig) -> anyhow::Result<()> {
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
            &format!("POSTGRES_PASSWORD={}", config.db_password),
            "-e",
            &format!("POSTGRES_DB={}", config.db_name),
            // The PostgreSQL image used to run the database.
            // E.G. "postgres:16-alpine".
            POSTGRES_IMAGE,
        ]),
        "Start a reuable Postgres container",
    )
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
pub(crate) fn postgres_is_ready(config: &PostgresConfig) -> bool {
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
pub(crate) fn wait_for_postgres(config: &PostgresConfig) -> anyhow::Result<()> {
    for _ in 0..60 {
        if postgres_is_ready(config) {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(500));
    }
    bail!("temporary Postgres container did not become ready")
}

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
pub(crate) fn reset_schema(config: &PostgresConfig) -> anyhow::Result<()> {
    run_psql(
        config,
        &["-c", "DROP SCHEMA IF EXISTS public CASCADE; CREATE SCHEMA public;"],
        "Rest temporary Postgres schema",
    )
}

pub(crate) fn apply_init_sql(config: &PostgresConfig) -> anyhow::Result<()> {
    let init_sql = config.root.join(INIT_SQL);

    run(
        Command::new("docker").args([
            "cp",
            init_sql.to_str().context("init.sql path is not valid UTF-8")?,
            &format!("{POSTGRES_CONTAINER}:/init.sql"),
        ]),
        "copy init.sql into a temporary Postgres container",
    )?;

    run_psql(config, &["-f", "/init.sql"], "apply init.sql")
}

/// Reads the `init.sql` file from the project root.
/// * Returns an error if the file cannot be read.
pub(crate) fn read_init_sql(root: &Path) -> anyhow::Result<String> {
    fs::read_to_string(root.join(INIT_SQL)).context("read init.sql")
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
pub(crate) fn run_tools_shell(config: &PostgresConfig, script: &str, action: &str) -> anyhow::Result<()> {
    let uid: u32 = nix::unistd::Uid::current().as_raw();
    let gid: u32 = nix::unistd::Gid::current().as_raw();

    run(
        Command::new("docker").current_dir(&config.root).args([
            "run",
            // Run container as host user/group so generated files are writable.
            "--user",
            &format!("{uid}:{gid}"),
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
            &format!("DATABASE_URL={}", config.database_url()),
            // The fixed name of the image the container will use.
            DOCKER_IMAGE,
            // `sh -c "<script>"` runs the supplied shell command
            "sh",
            "-c",
            script,
        ]),
        action,
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
pub(crate) fn run_psql(config: &PostgresConfig, args: &[&str], action: &str) -> anyhow::Result<()> {
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

/// Marks the temporary database as recently used and ensures one cleanup process is scheduled.
pub(crate) fn refresh_idle_timer(root: &Path) -> anyhow::Result<()> {
    write_timestamp(root.join(IDLE_TIMESTAMP_FILE))?;
    ensure_cleanup_process(root)
}

pub(crate) fn container_is_running(name: &str) -> bool {
    get_cmd_output(
        Command::new("docker").args(["inspect", "-f", "{{.State.Running}}", name]),
        "inspect Docker container",
    )
    .is_ok_and(|stdout| stdout.trim() == "true")
}

/// Attempts to delete a container with the given name using
/// `docker rm -f`. If the container does not exist or removal fails, the
/// error is ignored.
///
/// ## Documentation
/// See the official Docker CLI docs:
/// https://docs.docker.com/engine/reference/commandline/rm/
///
pub(crate) fn remove_container_if_exists(name: &str) {
    let _ = Command::new("docker")
        // `rm` (remove)
        // `-f` (force)
        .args(["rm", "-f", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
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
pub(crate) fn ensure_cleanup_process(root: &Path) -> anyhow::Result<()> {
    if is_cleanup_process_running(root) {
        return Ok(());
    }

    let pid_file: PathBuf = root.join(IDLE_PID_FILE);
    create_parent_dir(&pid_file)?;

    // Reuse the existing `xtask` executable.
    let mut command = Command::new(env::current_exe().context("locate xtask executable")?);

    let child = detach_command_io(command.arg("cleanup-schema-db").current_dir(root))
        // Execute the command as as a non-blocking subprocess.
        .spawn()
        .context("begin schema DB cleanup process")?;

    // Save the PID file with contents `child.id()`.
    fs::write(pid_file, child.id().to_string())?;

    Ok(())
}

/// Returns `true` if the `IDLE_PID_FILE` exists
/// and the PID still belongs to a running process.
pub(crate) fn is_cleanup_process_running(root: &Path) -> bool {
    fs::read_to_string(root.join(IDLE_PID_FILE)).is_ok_and(|pid| process_is_running(pid.trim()))
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
pub(crate) fn cleanup_container_if_idle() -> anyhow::Result<()> {
    let root: PathBuf = root_directory()?;

    loop {
        thread::sleep(IDLE_TIMEOUT);

        let should_cleanup = read_timestamp(root.join(IDLE_TIMESTAMP_FILE))?
            .map(|last_used| now_unix_seconds().saturating_sub(last_used) >= IDLE_TIMEOUT.as_secs())
            .unwrap_or(true);

        if should_cleanup {
            remove_container_if_exists(POSTGRES_CONTAINER);
            remove_file_if_exists(root.join(IDLE_PID_FILE));
            return Ok(());
        }
    }
}

/// Reads a timestamp (as `u64`) from a file
pub(crate) fn read_timestamp(path: PathBuf) -> anyhow::Result<Option<u64>> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().parse::<u64>().context("idle timestamp file is invalid"))
        .transpose()
}

pub(crate) fn write_timestamp(path: PathBuf) -> anyhow::Result<()> {
    create_parent_dir(&path)?;
    fs::write(path, now_unix_seconds().to_string())?;
    Ok(())
}
