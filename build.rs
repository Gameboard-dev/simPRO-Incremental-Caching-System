//! This build file generates the following
//! whenever the source file changes:
//! - `src/api.rs` from `openapi.yaml`
//! - `src/db/insertables.rs` from `src/db/table.rs`
//!
//! The project writes generated code into `src` instead of Cargo's
//! `OUT_DIR`:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/api.rs"));
//! ```
//!
//! With the `OUT_DIR` approach, the generated source file lives under `target/`
//! and varies between builds, for example:
//!
//! ```
//! target/debug/build/<crate>/out/api.rs
//! ```
//!
//! Writing into `src` ensures that the client is commit-friendly
//! and importable
//!

use std::fs;

const OPENAPI_YAML: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.yaml");

const API_RS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/src/api.rs");

const BUILD_CACHE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/target/build-cache.json"
);

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Build {
    openapi_hash: String,
    // more files here as needed
}

impl Build {
    fn regenerate() -> anyhow::Result<()> {
        let old: Build = Self::load();
        let new: Build = Self::current()?;
        if old.openapi_hash != new.openapi_hash {
            api::generate()?;
        }
        new.save()
    }
    fn load() -> Self {
        fs::read_to_string(BUILD_CACHE)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }
    fn current() -> anyhow::Result<Self> {
        Ok(Self {
            openapi_hash: Self::hash(OPENAPI_YAML)?,
        })
    }
    fn hash(path: &str) -> anyhow::Result<String> {
        use sha2::{Digest, Sha256};
        Ok(hex::encode(Sha256::digest(fs::read(path)?)))
    }
    fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) =
            std::path::Path::new(BUILD_CACHE).parent()
        {
            fs::create_dir_all(parent)?;
        }

        fs::write(
            BUILD_CACHE,
            serde_json::to_string(self)?,
        )?;
        Ok(())
    }
}

/// This script is compiled before compiling the package.
/// https://doc.rust-lang.org/cargo/reference/build-scripts.html
fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed={OPENAPI_YAML}");
    Build::regenerate()?;
    Ok(())
}

mod api {
    use super::*;
    use openapiv3::OpenAPI;
    use progenitor_middleware::{
        GenerationSettings, Generator,
    };
    use progenitor_middleware::{InterfaceStyle, TagStyle};
    pub(crate) fn generate() -> anyhow::Result<()> {
        // OpenAPIv3 gives us a structured parsed representation of the OpenAPI V3 File
        // This will rerun if the source 'openapi.yaml' changes.
        let openapi_file_contents: OpenAPI =
            serde_yaml::from_str(&fs::read_to_string(
                OPENAPI_YAML,
            )?)?;
        // Use builder-style endpoint calls instead of positional function arguments.
        // Consumers do not need to specify parameters that are not required or for which the API specifies defaults.
        // See: https://github.com/oxidecomputer/progenitor#buildrs
        let mut settings = GenerationSettings::new();
        settings.with_interface(InterfaceStyle::Builder);
        settings.with_tag(TagStyle::Merged);
        settings.with_derive("Debug");
        // Progenitor generates Rust tokens, not a formatted `.rs` source file.
        let tokens: proc_macro2::TokenStream =
            Generator::new(&settings)
                .generate_tokens(&openapi_file_contents)?;
        // `syn` checks that the tokens form valid Rust and builds a syntax tree from the tokens.
        let syntax_tree: syn::File = syn::parse2(tokens)?;
        // `prettyplease`` parses the syntax tree back into readable source code before `src/api.rs` is written to disk.
        let contents: String = prettyplease::unparse(&syntax_tree);
        fs::write(API_RS, contents)?;
        println!("cargo:info=Generated API written to '{API_RS}'");
        Ok(())
    }
}
