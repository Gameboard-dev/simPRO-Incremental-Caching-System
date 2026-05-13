//! (A)
//! Uses an `xtask` invocation and Docker runtime environment
//! to generate Diesel insertables and queryables
//! from a database SQL definition file (`init.sql`)
//!
//! ```sh
//! cargo watch --poll --delay 1 -w ./init.sql -x "run -p xtask -- generate-schema"
//! ```
//!
//! (B)
//! Deduplicate inline OpenAPI parameters and schemas into `components`:
//! - promotes inline schemas → `components.schemas`
//! - promotes inline parameters → `components.parameters`
//! - replaces them with `$ref`s
//!
//! ```sh
//! cargo run -p xtask -- dedupe-openapi openapi.json openapi.deduped.yaml
//! ```
#![allow(unused_imports)]
use docker::config::*;
use crate::{
    docker::diesel::{generate_schema, rebuild_tools_image},
    openapi::run_openapi_deduplication,
    docker::postgres::cleanup_container_if_idle,
};
use anyhow::Result;
use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

pub(crate) mod docker {
    pub(crate) mod config;
    pub(crate) mod postgres;
    pub(crate) mod diesel;
}
pub(crate) mod openapi;
pub(crate) mod misc;

const INIT_SQL: &str = "init.sql";

/// ```ignore,sh
/// cargo run -p xtask -- <COMMAND>
/// ```
fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(XtaskCommand::GenerateSchema) => generate_schema(),
        Some(XtaskCommand::Rebuild) => rebuild_tools_image(),
        Some(XtaskCommand::CleanupSchemaDb) => cleanup_container_if_idle(),
        Some(XtaskCommand::DedupeOpenapi { input, output }) => run_openapi_deduplication(input, output),
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

#[derive(Parser)]
#[command(name = "xtask", about = "Tasks", disable_version_flag = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<XtaskCommand>,
}

#[derive(Subcommand)]
#[command(rename_all = "kebab-case")]
enum XtaskCommand {
    /// This invokes an `xtask` invocation and Docker runtime environment
    /// to generate valid Diesel insertables and queryables
    /// from a database SQL definition file (`init.sql`)
    #[command(alias = "generate")]
    GenerateSchema,
    /// This removes any existing [`TOOLS_IMAGE`] and rebuilds it from the [`DOCKERFILE`]
    Rebuild,
    /// This invokes [`cleanup_container_if_idle`]
    CleanupSchemaDb,
    /// This deduplicates inline OpenAPI parameters and schemas into `components`:
    /// - promotes inline schemas → `components.schemas`
    /// - promotes inline parameters → `components.parameters`
    /// - replaces them with `$ref`s
    DedupeOpenapi {
        input: PathBuf,
        output: PathBuf,
    },
}
