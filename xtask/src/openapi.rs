use anyhow::Result;
use openapiv3::{
    Components, MediaType, OpenAPI, Operation, Parameter, PathItem, ReferenceOr, RequestBody, Response, Schema,
    SchemaKind, Type,
};
use serde::Serialize;
use std::{collections::HashMap, path::PathBuf};

use crate::misc::{fingerprint, pascal_case, read_json_or_yaml, singularize, write_json_or_yaml};

/// Reads an OpenAPI specification, moves repeated inline parameters and object
/// schemas into `components`, rewrites usages as `$ref`s, and writes the result.
pub(crate) fn run_openapi_deduplication(input: PathBuf, output: PathBuf) -> Result<()> {
    let mut api: OpenAPI = read_json_or_yaml(&input)?;
    Dedupe::new(&mut api)?.apply(&mut api)?;
    write_json_or_yaml(&output, &api)
}

/// In OpenAPI, reusable objects (schemas, parameters, etc.) live under
/// `components`, while inline definitions may appear throughout `paths`.
///
/// Deduplication requires:
/// - scanning `paths`
/// - extracting reusable objects
/// - inserting them into `components`
/// - rewriting references
///
/// This struct centralizes that process and maintains the necessary state.
/// Deduplication is based on structural equality via [`fingerprint`].
struct Dedupe {
    // The components to overwrite in the OpenAPI specification
    components: Components,
    /// A mapping of fingerprints to parameter names
    parameters: HashMap<String, String>,
    /// A mapping of fingerprints to schema names
    schemas: HashMap<String, String>,
}

impl Dedupe {
    /// * Creates a new deduplication context from an [`OpenAPI`] document taking ownership of  [`Components`].
    /// * This seeds lookup maps from existing components' fingerprints to avoid duplication.
    /// * The caller is responsible for reattaching [`Components`] via [`apply`].
    fn new(api: &mut OpenAPI) -> Result<Self> {
        let components = api.components.take().unwrap_or_default();
        Ok(Self {
            parameters: seed_registry(components.parameters.iter())?,
            schemas: seed_registry(components.schemas.iter())?,
            components,
        })
    }

    /// Applies deduplication to the entire [`OpenAPI`] document.
    fn apply(mut self, api: &mut OpenAPI) -> Result<()> {
        for (path, item) in &mut api.paths.paths {
            if let ReferenceOr::Item(item) = item {
                self.path_item(path, item)?;
            }
        }
        api.components = Some(self.components);
        Ok(())
    }

    /// Processes a single [`PathItem`].
    /// - Deduplicates path-level parameters
    /// - Iterates over operations
    fn path_item(&mut self, path: &str, item: &mut PathItem) -> Result<()> {
        self.parameters(&mut item.parameters)?;
        for operation in operations(item) {
            self.operation(path, operation)?;
        }
        Ok(())
    }

    /// Processes a single [`Operation`].
    /// - Deduplicates operation-level parameters
    /// - Promotes response schemas
    /// - Promotes request-body schemas
    fn operation(&mut self, path: &str, operation: &mut Operation) -> Result<()> {
        self.parameters(&mut operation.parameters)?;

        let response_name: String = schema_name_from_path(path);
        let request_name: String = format!("{response_name}Request");

        for response in operation
            .responses
            .responses
            .values_mut()
            .chain(operation.responses.default.iter_mut())
        {
            self.response(response, &response_name)?;
        }

        if let Some(body) = operation.request_body.as_mut() {
            self.request_body(body, &request_name)?;
        }

        Ok(())
    }

    /// Deduplicates parameters in place and promotes inline parameters
    /// to references in `components.parameters`.
    fn parameters(&mut self, parameters: &mut [ReferenceOr<Parameter>]) -> Result<()> {
        for parameter_ref in parameters {
            let ReferenceOr::Item(parameter) = parameter_ref else {
                continue;
            };
            let name: String = self.promote_parameter(parameter)?;
            *parameter_ref = component_ref("parameters", &name);
        }

        Ok(())
    }

    /// Processes a response object by visiting its media types
    /// e.g. ("content/json"). Each media type has an associated schema object.
    fn response(&mut self, response: &mut ReferenceOr<Response>, name: &str) -> Result<()> {
        let ReferenceOr::Item(response) = response else {
            return Ok(());
        };
        self.media_types(response.content.values_mut(), name)
    }

    /// Processes a request body by visiting its media types.
    fn request_body(&mut self, body: &mut ReferenceOr<RequestBody>, name: &str) -> Result<()> {
        let ReferenceOr::Item(body) = body else {
            return Ok(());
        };
        self.media_types(body.content.values_mut(), name)
    }

    /// Processes a collection of [`MediaType`] values containing a [`Schema`]
    fn media_types<'a>(&mut self, media_types: impl IntoIterator<Item = &'a mut MediaType>, name: &str) -> Result<()> {
        for media_type in media_types {
            self.media_type(media_type, name)?;
        }
        Ok(())
    }

    /// Processes a single [`MediaType`] schema
    /// handling both:
    /// - array schemas (promoting item types)
    /// - object schemas (promoting directly)
    fn media_type(&mut self, media_type: &mut MediaType, name: &str) -> Result<()> {
        let Some(schema_ref) = media_type.schema.as_mut() else {
            return Ok(());
        };
        if self.array_items(schema_ref, name)? {
            return Ok(());
        }
        if let ReferenceOr::Item(schema) = schema_ref {
            if is_object(schema) {
                let component = self.promote_schema(schema, name)?;
                *schema_ref = component_ref("schemas", &component);
            }
        }
        Ok(())
    }

    /// Handles array schemas (`type: array`) by promoting their item type.
    ///
    /// Returns `true` if the schema was an array (and handled), otherwise `false`.
    fn array_items(&mut self, schema_ref: &mut ReferenceOr<Schema>, name: &str) -> Result<bool> {
        let ReferenceOr::Item(schema) = schema_ref else {
            return Ok(false);
        };

        let SchemaKind::Type(Type::Array(array)) = &mut schema.schema_kind else {
            return Ok(false);
        };

        let component = match array.items.as_mut() {
            Some(ReferenceOr::Item(item)) if is_object(item) => Some(self.promote_schema(item, name)?),
            _ => None,
        };

        if let Some(component) = component {
            array.items = Some(component_ref("schemas", &component));
        }

        Ok(true)
    }

    /// Promotes a parameter into `components.parameters`.
    ///
    /// If an identical parameter already exists, its name is reused.
    fn promote_parameter(&mut self, parameter: &Parameter) -> Result<String> {
        let hash = fingerprint(parameter)?;

        if let Some(name) = self.parameters.get(&hash) {
            return Ok(name.clone());
        }

        // Use the sanitized parameter name as the preferred key
        let key = component_key(parameter.parameter_data_ref().name.as_str(), "parameter");

        // Reuse an identical component with the same name
        // Or if a conflicting (differently-hashed) component exists
        // fallback to a numerical suffix
        let key = unique_name(&key, "parameter", &hash, |candidate| {
            self.components.parameters.get(candidate).map(fingerprint).transpose()
        })?;

        self.components
            .parameters
            .insert(key.clone(), ReferenceOr::Item(parameter.clone()));

        self.parameters.insert(hash, key.clone());
        Ok(key)
    }

    /// Promotes a schema into `components.schemas`.
    ///
    /// If an identical schema already exists, its name is reused.
    fn promote_schema(&mut self, schema: &Schema, preferred: &str) -> Result<String> {
        let hash = fingerprint(schema)?;

        if let Some(name) = self.schemas.get(&hash) {
            return Ok(name.clone());
        }

        let name = unique_name(preferred, "Schema", &hash, |candidate| {
            self.components.schemas.get(candidate).map(fingerprint).transpose()
        })?;

        self.components
            .schemas
            .insert(name.clone(), ReferenceOr::Item(schema.clone()));

        self.schemas.insert(hash, name.clone());
        Ok(name)
    }
}

/// In OpenAPI, each path may define up to eight operations:
/// - GET, PUT, POST, DELETE
/// - OPTIONS, HEAD, PATCH, TRACE
///
/// Each operation is stored as an `Option<Operation>` on [`PathItem`].
///
/// This yields only the operations that are defined.
fn operations(item: &mut PathItem) -> impl Iterator<Item = &mut Operation> + '_ {
    [
        item.get.as_mut(),
        item.put.as_mut(),
        item.post.as_mut(),
        item.delete.as_mut(),
        item.options.as_mut(),
        item.head.as_mut(),
        item.patch.as_mut(),
        item.trace.as_mut(),
    ]
    .into_iter()
    .flatten()
}

/// Returns a [`HashMap`] of component fingerprint to component name.
///
/// During deduplication, inline parameters and schemas are promoted into
/// `components` and referenced via `$ref`. To avoid creating duplicates,
/// we need a way to detect when a structurally identical component already
/// exists.
///
/// - Each component value is serialized into a stable JSON representation
///   using [`fingerprint`]
///
/// - The fingerprint hash key corresponds to the component name (in `components`) stored
///   as the value
///
/// Existing `$ref` entries are ignored because they do not contain concrete data to
/// fingerprint.
fn seed_registry<'a, T>(
    items: impl Iterator<Item = (&'a String, &'a ReferenceOr<T>)>,
) -> Result<HashMap<String, String>>
where
    T: Serialize + 'a,
{
    items
        .filter_map(|(name, item)| match item {
            ReferenceOr::Item(value) => Some(fingerprint(value).map(|key: String| (key, name.clone()))),
            ReferenceOr::Reference { .. } => None,
        })
        .collect()
}

/// Finds a free component name, appending numeric suffixes on conflicts.
///
/// If the preferred name already exists with identical content, it is reused.
///
/// If the name exists with different content, `2`, `3`, etc. are appended.
fn unique_name(
    preferred: &str,
    fallback: &str,
    wanted_fingerprint: &str,
    mut existing_fingerprint: impl FnMut(&str) -> Result<Option<String>>,
) -> Result<String> {
    let base: String = component_key(preferred, fallback);
    let mut candidate: String = base.clone();

    for suffix in 1.. {
        match existing_fingerprint(&candidate)? {
            None => return Ok(candidate),
            Some(existing) if existing == wanted_fingerprint => return Ok(candidate),
            Some(_) => candidate = format!("{base}{}", suffix + 1),
        }
    }

    unreachable!("unbounded suffix iterator never ends")
}

fn component_ref<T>(section: &str, name: &str) -> ReferenceOr<T> {
    ReferenceOr::Reference {
        reference: format!("#/components/{section}/{name}"),
    }
}

/// During deduplication, only object-shaped schemas are promoted into
/// `components.schemas`. Primitive types (e.g. `string`, `number`) and simple
/// arrays are left inline, but structured objects are extracted and reused.
///
/// ```ignore,json
/// { type: "object", properties: { ... } }
/// ```
fn is_object(schema: &Schema) -> bool {
    match &schema.schema_kind {
        SchemaKind::Type(Type::Object(_)) => true,
        SchemaKind::Any(any) => any.typ.as_deref() == Some("object") || !any.properties.is_empty(),
        _ => false,
    }
}

/// Derives a component schema name from the last non-parameter path segment
/// using an English singularization library and `heck` for PascalCase.
/// ```ignore
/// - `/api/v1.0/companies/{companyID}/customers/` -> `Customer`
/// - `/api/v1.0/companies/{companyID}/accounts/journals/` -> `Journal`
/// ```
fn schema_name_from_path(path: &str) -> String {
    path.split('/')
        .filter(|part| !part.is_empty() && !part.starts_with('{'))
        .last()
        .map(singularize)
        .map(|name| pascal_case(&name))
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Schema".to_string())
}

/// OpenAPI component names (e.g. under `components.schemas` or
/// `components.parameters`) are used in `$ref` strings.
///
/// To keep these references readable and valid, component keys should avoid
/// special characters such as spaces, slashes, or punctuation.
///
/// This function sanitizes a string by:
/// - preserving ASCII alphanumeric characters (`a-z`, `A-Z`, `0-9`)
/// - preserving `.`, `_`, and `-`
/// - replacing all other characters with `_`
///
/// ```ignore,rust
/// assert_eq!(component_key("cost center", "Schema"), "cost_center");
/// assert_eq!(component_key("accounts/journals", "Schema"), "accounts_journals");
/// assert_eq!(component_key("@#$%", "Schema"), "____");
/// assert_eq!(component_key("", "Schema"), "Schema");
/// ```
fn component_key(value: &str, fallback: &str) -> String {
    let key: String = value
        .chars()
        .map(|c| match c {
            c if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') => c,
            _ => '_',
        })
        .collect();

    if key.is_empty() { fallback.into() } else { key }
}
