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

const OPENAPI_YAML: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/openapi.yaml");

const API_RS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/src/api.rs");

const BUILD_CACHE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/target/build-cache.json");

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
        if let Some(parent) = std::path::Path::new(BUILD_CACHE).parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(BUILD_CACHE, serde_json::to_string(self)?)?;
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
    use progenitor_middleware::{GenerationSettings, Generator};
    use progenitor_middleware::{InterfaceStyle, TagStyle};
    pub(crate) fn generate() -> anyhow::Result<()> {
        // OpenAPIv3 gives us a structured parsed representation of the OpenAPI V3 File
        // This will rerun if the source 'openapi.yaml' changes.
        let openapi_file_contents: OpenAPI = serde_yaml::from_str(&fs::read_to_string(OPENAPI_YAML)?)?;
        // Use builder-style endpoint calls instead of positional function arguments.
        // Consumers do not need to specify parameters that are not required or for which the API specifies defaults.
        // See: https://github.com/oxidecomputer/progenitor#buildrs
        let mut settings = GenerationSettings::new();
        settings.with_interface(InterfaceStyle::Builder);
        settings.with_tag(TagStyle::Merged);
        settings.with_derive("Debug");
        // Progenitor generates Rust tokens, not a formatted `.rs` source file.
        let tokens: proc_macro2::TokenStream = Generator::new(&settings).generate_tokens(&openapi_file_contents)?;
        // `syn` checks that the tokens form valid Rust and builds a syntax tree from the tokens.
        let mut syntax_tree: syn::File = syn::parse2(tokens)?;
        syntax_tree.items.push(syn::parse_quote! {
            pub trait Columns {
                const COLUMNS: &'static [&'static str];
            }
        });
        // Parse a COLUMNS constant value for 'columns?' query string in GET requests
        syntax_tree.items.extend(columns::generate(&syntax_tree)?);
        // `prettyplease`` parses the syntax tree back into readable source code before `src/api.rs` is written to disk.
        let contents: String = prettyplease::unparse(&syntax_tree);
        fs::write(API_RS, contents)?;
        println!("cargo:info=Generated API written to '{API_RS}'");
        Ok(())
    }
}

mod columns {
    use quote::quote;

    pub(crate) fn generate(file: &syn::File) -> anyhow::Result<Vec<syn::Item>> {
        let Some(types) = file.items.iter().find_map(types_mod) else {
            return Ok(vec![]);
        };

        types
            .content
            .as_ref()
            .into_iter()
            .flat_map(|(_, items)| items)
            .filter_map(named_struct)
            .filter_map(|(name, fields)| {
                let columns = fields.named.iter().filter_map(rename).collect::<Vec<_>>();
                (!columns.is_empty()).then_some((name, columns))
            })
            .map(|(name, columns)| -> anyhow::Result<syn::Item> {
                Ok(syn::parse2(quote! {
                    impl Columns for types::#name {
                        const COLUMNS: &'static [&'static str] = &[#(#columns,)*];
                    }
                })?)
            })
            .collect()
    }

    /// Find `pub mod types { ... }`. API schemas are nested in here.
    fn types_mod(item: &syn::Item) -> Option<&syn::ItemMod> {
        match item {
            syn::Item::Mod(m) if m.ident == "types" => Some(m),
            _ => None,
        }
    }

    /// Keep only `struct Foo { ... }`. Ignore unnamed tuple structs.
    fn named_struct(item: &syn::Item) -> Option<(&syn::Ident, &syn::FieldsNamed)> {
        match item {
            syn::Item::Struct(s) => match &s.fields {
                syn::Fields::Named(fields) => Some((&s.ident, fields)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Extract `Bar` from `#[serde(rename = "Bar")]`.
    fn rename(field: &syn::Field) -> Option<String> {
        field.attrs.iter().find_map(|attr| {
            attr.path().is_ident("serde").then(|| {
                let mut out = None;
                attr.parse_nested_meta(|meta| {
                    if meta.path.is_ident("rename") {
                        out = Some(meta.value()?.parse::<syn::LitStr>()?.value());
                    }
                    Ok(())
                }).ok()?;
                out
            })?
        })
    }
}
