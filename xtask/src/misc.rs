use anyhow::{Context, bail};
use serde::de::DeserializeOwned;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Output, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::docker::config::DOCKER_NETWORK;

/// Returns the Unix timestamp in seconds.
///
/// Unix time is the number of seconds elapsed since:
///
/// ```text
/// 1970-01-01 00:00:00 UTC
/// ```
pub(crate) fn now_unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("System clock is before UNIX epoch")
        .as_secs()
}

/// Creates the parent directory for `path` if it has one.
///
/// ```ignore
/// create_parent_dir(Path::new("target/generated/schema.json"))?;
/// ```
///
/// Returns an error if the parent directory cannot be created.
pub(crate) fn create_parent_dir(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    Ok(())
}

/// Removes a file if it exists.
pub(crate) fn remove_file_if_exists(path: impl AsRef<Path>) {
    let _ = fs::remove_file(path);
}

/// `CARGO_MANIFEST_DIR` is set at compile time and points to the directory
/// containing this crate’s `Cargo.toml`, which for this project is the
/// `xtask/` directory.
///
/// This function returns the parent directory of `xtask/`, which is assumed
/// to be the workspace or project root.
pub(crate) fn root_directory() -> anyhow::Result<PathBuf> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(Path::to_path_buf)
        .context("xtask directory has no parent")
}

/// Reads a required environment variable.
/// * Returns an error if the variable is not present.
pub(crate) fn require_env(name: &str) -> anyhow::Result<String> {
    env::var(name).with_context(|| format!("{name} is missing from .env or the environment"))
}

/// Converts a `snake_case` identifier into `PascalCase`.
///
/// ```ignore
/// assert_eq!(snake_to_pascal_case("customer_id"), "CustomerId");
/// assert_eq!(snake_to_pascal_case("__api_schema"), "ApiSchema");
/// ```
pub(crate) fn snake_to_pascal_case(value: &str) -> String {
    value
        .split('_')
        .filter(|part| !part.is_empty())
        .map(capitalize)
        .collect()
}

/// Converts the first character of `value` to uppercase and leaves the
/// remaining characters unchanged.
pub(crate) fn capitalize(value: &str) -> String {
    let Some(first) = value.chars().next() else {
        return String::new();
    };

    first.to_uppercase().chain(value[first.len_utf8()..].chars()).collect()
}

/// Classifies a string as PascalCase-like.
/// - The first character must be uppercase ASCII
/// - The value must not contain underscores
pub(crate) fn is_pascal_case(value: &str) -> bool {
    value.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && !value.contains('_')
}

/// This function checks whether a Docker network named
/// [`DOCKER_UTILITY_NETWORK`] exists. If it already exists, nothing happens.
/// Otherwise, the network is created.
///
/// Without a Docker network, containers communicate through ports
/// exposed on the host machine, such as `localhost`. This is less
/// straightforward and portable because host networking differs between
/// Linux, WSL, Windows, and macOS.
pub(crate) fn ensure_docker_network() -> anyhow::Result<()> {
    if docker_network_exists(DOCKER_NETWORK) {
        return Ok(());
    }
    run(
        &mut docker_command(["network", "create", DOCKER_NETWORK]),
        "create Docker network",
    )
}

/// Checks whether a Docker network exists.
/// Any failure returns `false`.
///
/// ```ignore
/// docker network inspect <NETWORK_NAME>
/// ```
pub(crate) fn docker_network_exists(name: &str) -> bool {
    command_succeeds(&mut docker_command(["network", "inspect", name]))
}

/// Checks whether a Docker image exists locally.
/// Any failure returns `false`.
///
/// ```ignore
/// docker image inspect <IMAGE_NAME>
/// ```
pub(crate) fn docker_image_exists(name: &str) -> bool {
    command_succeeds(&mut docker_command(["image", "inspect", name]))
}

/// Creates a Docker command with the provided arguments.
pub(crate) fn docker_command<const N: usize>(args: [&str; N]) -> Command {
    let mut command = Command::new("docker");
    command.args(args);
    command
}

/// Every process has three standard I/O streams:
///
/// - `stdin` is where the program reads input from
/// - `stdout` is where normal output is written
/// - `stderr` is where error messages are written
///
/// This function redirects all three to `Stdio::null()`, which is equivalent
/// to sending them to `/dev/null` on Unix or `NUL` on Windows allowing
/// the command to run in the background.
pub(crate) fn detach_command_io(command: &mut Command) -> &mut Command {
    command.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
}

/// Checks whether a process with the given process identifier (PID) is still running.
///
/// On Windows, this uses:
///
/// ```ignore
/// tasklist /FI "PID eq <pid>"
/// ```
///
/// On Unix-like systems, including Linux, macOS, and WSL, this uses:
///
/// ```ignore
/// kill -0 <pid>
/// ```
///
/// `kill -0` does not kill the process. It only asks the operating system
/// whether a process with that PID exists and can be signalled.
pub(crate) fn process_is_running(pid: &str) -> bool {
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
                output.status.success() && String::from_utf8_lossy(&output.stdout).contains(&pid.to_string())
            })
            .unwrap_or(false)
    }

    #[cfg(not(windows))]
    {
        command_succeeds(Command::new("kill").args(["-0", &pid.to_string()]))
    }
}

/// Executes a command and checks only for success or failure.
///
/// ```ignore
/// run(Command::new("docker").args(["build", "."]), "build image")?;
/// ```
///
pub(crate) fn run(command: &mut Command, action: &str) -> anyhow::Result<()> {
    println!("→ {action}");

    let status: ExitStatus = command.status().with_context(|| format!("failed to {action}"))?;

    if !status.success() {
        bail!("failed to {action} with status {status}")
    }

    return Ok(());
}

/// Runs a command and returns whether it exited successfully.
///
/// ```ignore,rust
/// if command_succeeds(Command::new("docker").args(["image", "inspect", image])) {
///     println!("image exists");
/// }
/// ```
pub(crate) fn command_succeeds(command: &mut Command) -> bool {
    command
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Executes a command and captures its standard output.
///
/// The `action` string is used to provide contextual error messages.
///
/// ```ignore,rust
/// let output = get_cmd_output(
///     Command::new("docker").args(["inspect", "my-container"]),
///     "inspect container",
/// )?;
/// ```
///
/// Use this when you need to make decisions based on command output.
///
/// Use [`run`] instead when you only care about success or failure and want
/// streaming output in the terminal.
pub(crate) fn get_cmd_output(command: &mut Command, action: &str) -> anyhow::Result<String> {
    let output: Output = command.output().with_context(|| format!("failed to {action}"))?;

    if !output.status.success() {
        bail!(
            "failed to {action}\n\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        );
    }

    String::from_utf8(output.stdout).context("command stdout was not valid UTF-8")
}

#[derive(Clone, Copy)]
enum DataFormat {
    Json,
    Yaml,
}

impl DataFormat {
    fn from_path(path: &Path) -> anyhow::Result<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Ok(Self::Json),
            Some("yaml" | "yml") => Ok(Self::Yaml),
            Some(ext) => bail!(
                "unsupported file extension .{ext} for {}; expected .json, .yaml, or .yml",
                path.display()
            ),
            None => bail!(
                "missing file extension for {}; expected .json, .yaml, or .yml",
                path.display()
            ),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Json => "JSON",
            Self::Yaml => "YAML",
        }
    }
}

/// Reads a JSON or YAML file into a strongly typed value.
pub(crate) fn read_json_or_yaml<T>(path: &Path) -> anyhow::Result<T>
where
    T: DeserializeOwned,
{
    let format: DataFormat = DataFormat::from_path(path)?;
    let contents: String = fs::read_to_string(path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    match format {
        DataFormat::Json => serde_json::from_str::<T>(&contents)
            .with_context(|| format!("failed to parse {} {}", format.name(), path.display())),
        
        DataFormat::Yaml => serde_yaml::from_str::<T>(&contents)
            .with_context(|| format!("failed to parse {} {}", format.name(), path.display())),
    }
}
/// Writes a serializable value as either JSON or YAML depending on the extension.
pub(crate) fn write_json_or_yaml<T>(path: &Path, value: &T) -> anyhow::Result<()>
where
    T: serde::Serialize,
{
    let format: DataFormat = DataFormat::from_path(path)?;
    let contents: String = match format {
        DataFormat::Json => serde_json::to_string_pretty(value)?,
        DataFormat::Yaml => serde_yaml::to_string(value)?,
    };
    create_parent_dir(path)?;
    fs::write(path, contents).with_context(|| format!("Failed to write {}", path.display()))
}

/// Returns a stable JSON fingerprint for a serializable value
///
/// This is used for structural deduplication when the type itself does not
/// provide convenient equality or hashing.
///
pub(crate) fn fingerprint<T>(value: &T) -> anyhow::Result<String>
where
    T: serde::Serialize,
{
    Ok(serde_json::to_string(&serde_json::to_value(value)?)?)
}

/// Converts an English noun to its singular form.
///
/// ```ignore
/// assert_eq!(singularize("customers"), "customer");
/// assert_eq!(singularize("companies"), "company");
/// assert_eq!(singularize("journals"), "journal");
/// ```
pub(crate) fn singularize(value: &str) -> String {
    pluralizer::pluralize(value, 1, false)
}

/// Converts a `snake_case`, `kebab-case`, path segment, or mixed identifier
/// into PascalCase / UpperCamelCase.
///
/// This uses the `heck` crate instead of maintaining local case-conversion
/// rules.
///
/// ```ignore
/// assert_eq!(pascal_case("customer_id"), "CustomerId");
/// assert_eq!(pascal_case("cost-centers"), "CostCenters");
/// assert_eq!(pascal_case("journal entries"), "JournalEntries");
/// ```
pub(crate) fn pascal_case(value: &str) -> String {
    use heck::ToUpperCamelCase;
    value.to_upper_camel_case()
}

pub(crate) fn remove_unused_lifetimes_from_structs(path: &str) -> anyhow::Result<()> {
    use anyhow::Context;
    use std::collections::BTreeSet;
    use syn::visit::Visit;
    use syn::visit_mut::{self, VisitMut};
    struct RemoveUnusedLifetimes;

    impl VisitMut for RemoveUnusedLifetimes {
        fn visit_item_struct_mut(&mut self, item: &mut syn::ItemStruct) {
            visit_mut::visit_item_struct_mut(self, item);
            let used_lifetimes: BTreeSet<String> = lifetimes_used_in_fields(&item.fields);

            item.generics.params = item
                .generics
                .params
                .clone()
                .into_iter()
                .filter(|param: &syn::GenericParam| match param {
                    syn::GenericParam::Lifetime(lifetime) => {
                        used_lifetimes.contains(&lifetime.lifetime.ident.to_string())
                    }
                    _ => true,
                })
                .collect();
        }
    }

    fn lifetimes_used_in_fields(fields: &syn::Fields) -> BTreeSet<String> {
        struct LifetimeCollector {
            lifetimes: BTreeSet<String>,
        }
        impl<'ast> Visit<'ast> for LifetimeCollector {
            fn visit_lifetime(&mut self, lifetime: &'ast syn::Lifetime) {
                self.lifetimes.insert(lifetime.ident.to_string());
            }
        }
        let mut collector: LifetimeCollector = LifetimeCollector {
            lifetimes: Default::default(),
        };
        collector.visit_fields(fields);
        collector.lifetimes
    }

    let source: String = std::fs::read_to_string(path).with_context(|| format!("read generated Rust file {path}"))?;

    let mut file: syn::File = syn::parse_file(&source).with_context(|| format!("parse generated Rust file {path}"))?;

    RemoveUnusedLifetimes.visit_file_mut(&mut file);

    std::fs::write(path, prettyplease::unparse(&file))
        .with_context(|| format!("write postprocessed Rust file {path}"))?;

    Ok(())
}