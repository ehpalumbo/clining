//! Maps an OpenAPI 3.0 spec into the domain `ApiModel`.

use std::collections::{BTreeMap, HashMap, HashSet};

use super::spec::{
    MediaType, OpenApi30Spec, Operation, Parameter, PathItem, RequestBody, Response,
};
use crate::domain::command_name::{cli_name, command_name, disambiguate};
use crate::domain::errors::DomainError;
use crate::domain::model::{
    ApiModel, ApiOperation, ApiOperationGroup, BodySpec, CompositeKind, HttpMethod, ModelVersion,
    Param, ResponseSpec, SchemaSpec,
};
use crate::domain::ports::OpenApiParser;

/// Adapter that parses OpenAPI 3.0.x JSON into a domain `ApiModel`.
pub struct Parser;

impl OpenApiParser for Parser {
    fn parse(&self, bytes: &[u8]) -> Result<ApiModel, DomainError> {
        let root: serde_json::Value =
            serde_json::from_slice(bytes).map_err(|e| DomainError::InvalidSpec {
                message: format!("malformed JSON: {e}"),
            })?;
        Self::check_version(&root)?;
        let spec: OpenApi30Spec =
            serde_json::from_value(root).map_err(|e| DomainError::InvalidSpec {
                message: format!("malformed spec: {e}"),
            })?;
        Ok(Self::build_model(spec))
    }
}

impl Parser {
    fn check_version(root: &serde_json::Value) -> Result<(), DomainError> {
        if let Some(version) = root.get("openapi").and_then(serde_json::Value::as_str) {
            if version.starts_with("3.0") {
                return Ok(());
            }
            return Err(DomainError::InvalidSpec {
                message: format!("unsupported OpenAPI version '{version}' (expected 3.0.x)"),
            });
        }
        if let Some(version) = root.get("swagger").and_then(serde_json::Value::as_str) {
            return Err(DomainError::InvalidSpec {
                message: format!(
                    "unsupported OpenAPI version '{version}' (Swagger 2.0 is not supported; expected 3.0.x)"
                ),
            });
        }
        Err(DomainError::InvalidSpec {
            message: "not an OpenAPI document: missing 'openapi' field".to_owned(),
        })
    }

    fn build_model(spec: OpenApi30Spec) -> ApiModel {
        let base_url = spec
            .servers
            .as_ref()
            .and_then(|s| s.first())
            .map(|s| s.url.clone())
            .unwrap_or_default();
        let name = spec
            .info
            .as_ref()
            .and_then(|i| i.title.clone())
            .unwrap_or_default();

        let schema_registry: BTreeMap<String, SchemaSpec> = spec
            .components
            .as_ref()
            .map(|components| {
                components
                    .schemas
                    .iter()
                    .map(|(schema_name, value)| (schema_name.clone(), Self::parse_schema(value)))
                    .collect()
            })
            .unwrap_or_default();

        let tag_descriptions: HashMap<&str, &str> = spec
            .tags
            .as_ref()
            .into_iter()
            .flatten()
            .filter_map(|t| t.description.as_deref().map(|d| (t.name.as_str(), d)))
            .collect();

        let mut operation_groups: Vec<ApiOperationGroup> = Vec::new();
        let mut operation_group_index: BTreeMap<String, usize> = BTreeMap::new();
        for (path, item) in &spec.paths {
            for (method, op) in Self::operations(item) {
                let group_name = op
                    .tags
                    .as_ref()
                    .and_then(|t| t.first())
                    .cloned()
                    .unwrap_or_else(|| "default".to_owned());
                let idx = match operation_group_index.get(&group_name) {
                    Some(i) => *i,
                    None => {
                        let i = operation_groups.len();
                        operation_groups.push(ApiOperationGroup {
                            name: group_name.clone(),
                            description: tag_descriptions
                                .get(group_name.as_str())
                                .map(|d| (*d).to_owned()),
                            operations: Vec::new(),
                        });
                        operation_group_index.insert(group_name.clone(), i);
                        i
                    }
                };
                operation_groups[idx]
                    .operations
                    .push(Self::build_operation(op, item, method, path));
            }
        }
        for operation_group in &mut operation_groups {
            let names: Vec<String> = operation_group
                .operations
                .iter()
                .map(|o| o.name.clone())
                .collect();
            let disambiguated = disambiguate(names);
            for (operation, name) in operation_group.operations.iter_mut().zip(disambiguated) {
                operation.name = name;
            }
        }

        ApiModel {
            name,
            base_url,
            version: ModelVersion::V1,
            schema_registry,
            operation_groups,
        }
    }

    fn operations(item: &PathItem) -> Vec<(HttpMethod, &Operation)> {
        let mut out = Vec::new();
        if let Some(op) = &item.get {
            out.push((HttpMethod::Get, op));
        }
        if let Some(op) = &item.put {
            out.push((HttpMethod::Put, op));
        }
        if let Some(op) = &item.post {
            out.push((HttpMethod::Post, op));
        }
        if let Some(op) = &item.delete {
            out.push((HttpMethod::Delete, op));
        }
        if let Some(op) = &item.patch {
            out.push((HttpMethod::Patch, op));
        }
        if let Some(op) = &item.head {
            out.push((HttpMethod::Head, op));
        }
        if let Some(op) = &item.options {
            out.push((HttpMethod::Options, op));
        }
        if let Some(op) = &item.trace {
            out.push((HttpMethod::Trace, op));
        }
        out
    }

    fn build_operation(
        op: &Operation,
        item: &PathItem,
        method: HttpMethod,
        path: &str,
    ) -> ApiOperation {
        let name = command_name(op.operation_id.as_deref(), method, path);
        let params = Self::merged_parameters(item, op);
        let path_params = params
            .iter()
            .filter(|p| p.location == "path")
            .map(Self::to_param)
            .collect();
        let query_params = params
            .iter()
            .filter(|p| p.location == "query")
            .map(Self::to_param)
            .collect();
        let request_body = op.request_body.as_ref().map(Self::to_body_spec);
        let responses = op
            .responses
            .as_ref()
            .map(Self::to_response_specs)
            .unwrap_or_default();
        ApiOperation {
            name,
            summary: op.summary.clone(),
            method,
            path: path.to_owned(),
            path_params,
            query_params,
            request_body,
            responses,
        }
    }

    fn merged_parameters(item: &PathItem, op: &Operation) -> Vec<Parameter> {
        let mut out: Vec<Parameter> = Vec::new();
        let mut seen: HashSet<(String, String)> = HashSet::new();
        for p in item
            .parameters
            .iter()
            .flatten()
            .chain(op.parameters.iter().flatten())
        {
            let key = (p.name.clone(), p.location.clone());
            if seen.insert(key) {
                out.push(p.clone());
            } else if let Some(slot) = out
                .iter_mut()
                .find(|q| q.name == p.name && q.location == p.location)
            {
                *slot = p.clone();
            }
        }
        out
    }

    fn to_param(p: &Parameter) -> Param {
        Param {
            name: p.name.clone(),
            canonical_name: cli_name(&p.name),
            required: p.required.unwrap_or(false),
            description: p.description.clone(),
        }
    }

    fn to_body_spec(rb: &RequestBody) -> BodySpec {
        let (content_type, media) = match rb.content.iter().next() {
            Some((ct, mt)) => (ct.clone(), mt),
            None => ("application/json".to_owned(), &MediaType { schema: None }),
        };
        let schema = media.schema.as_ref().map(Self::parse_schema);
        BodySpec {
            required: rb.required.unwrap_or(false),
            content_type,
            schema,
        }
    }

    /// Flattens an operation's responses into one `ResponseSpec` per
    /// (status code, content type).
    fn to_response_specs(responses: &BTreeMap<String, Response>) -> Vec<ResponseSpec> {
        let mut out = Vec::new();
        for (status_code, response) in responses {
            for (content_type, media) in response.content.iter().flatten() {
                out.push(ResponseSpec {
                    status_code: status_code.clone(),
                    content_type: content_type.clone(),
                    schema: media.schema.as_ref().map(Self::parse_schema),
                });
            }
        }
        out
    }

    /// Converts a raw JSON schema into the typed `SchemaSpec`. Local
    /// `#/components/schemas/...` refs become `Ref` (never inlined, so cyclic
    /// schemas are safe at learn time); object, array, and composite nodes retain
    /// their navigation structure and store only unmodeled metadata in `extra_json`;
    /// primitive and unknown nodes preserve complete raw JSON payloads.
    fn parse_schema(value: &serde_json::Value) -> SchemaSpec {
        let obj = match value.as_object() {
            Some(obj) => obj,
            None => return Self::unknown(value),
        };
        if let Some(ref_str) = obj.get("$ref").and_then(serde_json::Value::as_str) {
            return match Self::local_schema_ref_name(ref_str) {
                Some(ref_id) => SchemaSpec::Ref { ref_id },
                None => Self::unknown(value),
            };
        }
        for composite_kind in [
            CompositeKind::AllOf,
            CompositeKind::OneOf,
            CompositeKind::AnyOf,
        ] {
            if let Some(branches) = obj
                .get(composite_kind.keyword())
                .and_then(serde_json::Value::as_array)
            {
                return Self::composite_schema(obj, composite_kind, branches);
            }
        }
        match obj.get("type").and_then(serde_json::Value::as_str) {
            Some("object") => return Self::object_schema(obj),
            Some("array") => return Self::array_schema(obj),
            Some("integer" | "number" | "string" | "boolean") => {
                return SchemaSpec::Primitive {
                    raw_json: value.to_string(),
                };
            }
            _ => {}
        }
        if obj.contains_key("properties") {
            return Self::object_schema(obj);
        }
        Self::unknown(value)
    }

    fn unknown(value: &serde_json::Value) -> SchemaSpec {
        SchemaSpec::Unknown {
            raw_json: value.to_string(),
        }
    }

    fn composite_schema(
        obj: &serde_json::Map<String, serde_json::Value>,
        composite_kind: CompositeKind,
        branches: &[serde_json::Value],
    ) -> SchemaSpec {
        let schemas = branches.iter().map(Self::parse_schema).collect();
        let mut extra = obj.clone();
        extra.remove(composite_kind.keyword());
        let extra_json = Self::extra_json_string(extra);
        SchemaSpec::Composite {
            composite_kind,
            schemas,
            extra_json,
        }
    }

    /// Maps an object schema to `SchemaSpec::Object`, recursively parsing each property
    /// and storing only unmodeled extra metadata in `extra_json`.
    fn object_schema(obj: &serde_json::Map<String, serde_json::Value>) -> SchemaSpec {
        let properties = obj
            .get("properties")
            .and_then(serde_json::Value::as_object)
            .map(|props| {
                props
                    .iter()
                    .map(|(property_name, prop)| (property_name.clone(), Self::parse_schema(prop)))
                    .collect()
            })
            .unwrap_or_default();
        let mut extra = obj.clone();
        extra.remove("properties");
        if extra.get("type").and_then(serde_json::Value::as_str) == Some("object") {
            extra.remove("type");
        }
        let extra_json = Self::extra_json_string(extra);
        SchemaSpec::Object {
            properties,
            extra_json,
        }
    }

    /// Maps an array schema to `SchemaSpec::Array`, recursively parsing items
    /// and storing only unmodeled extra metadata in `extra_json`.
    fn array_schema(obj: &serde_json::Map<String, serde_json::Value>) -> SchemaSpec {
        let items = obj.get("items").map(Self::parse_schema).map(Box::new);
        let mut extra = obj.clone();
        extra.remove("items");
        if extra.get("type").and_then(serde_json::Value::as_str) == Some("array") {
            extra.remove("type");
        }
        let extra_json = Self::extra_json_string(extra);
        SchemaSpec::Array { items, extra_json }
    }

    fn extra_json_string(extra: serde_json::Map<String, serde_json::Value>) -> Option<String> {
        if extra.is_empty() {
            None
        } else {
            Some(serde_json::Value::Object(extra).to_string())
        }
    }

    /// Trailing name for a local `#/components/schemas/<name>` reference.
    fn local_schema_ref_name(ref_str: &str) -> Option<String> {
        ref_str
            .strip_prefix("#/components/schemas/")
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r##"{
        "openapi": "3.0.0",
        "info": { "title": "Petstore", "version": "1.0.0" },
        "servers": [{ "url": "https://api.example.com/v1" }],
        "tags": [
            { "name": "pets", "description": "Everything about your pets" },
            { "name": "store", "description": "Access to store orders" }
        ],
        "components": {
            "schemas": {
                "Order": {
                    "type": "object",
                    "required": ["id"],
                    "properties": {
                        "id": { "type": "integer", "description": "Order id", "minimum": 1 },
                        "petId": { "type": "integer" },
                        "quantity": { "type": "integer", "description": "Number of units", "minimum": 1, "default": 1 }
                    }
                }
            }
        },
        "paths": {
            "/pets": {
                "parameters": [
                    { "name": "sort", "in": "query", "required": false },
                    { "name": "limit", "in": "query", "required": true }
                ],
                "get": {
                    "operationId": "listPets",
                    "summary": "List all pets",
                    "tags": ["pets"],
                    "parameters": [
                        { "name": "limit", "in": "query", "required": false }
                    ]
                },
                "post": {
                    "operationId": "createPet",
                    "tags": ["pets"],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": { "schema": { "type": "object" } }
                        }
                    },
                    "responses": {
                        "200": {
                            "content": {
                                "application/json": { "schema": { "type": "object" } }
                            }
                        }
                    }
                }
            },
            "/pets/{petId}": {
                "get": {
                    "summary": "Get a pet by id",
                    "parameters": [
                        { "name": "petId", "in": "path", "required": true, "description": "The numeric pet id" },
                        { "name": "X-API-Key", "in": "header" }
                    ]
                }
            },
            "/store/orders": {
                "post": {
                    "operationId": "placeOrder",
                    "tags": ["store"],
                    "requestBody": {
                        "required": true,
                        "content": {
                            "application/json": { "schema": { "$ref": "#/components/schemas/Order" } }
                        }
                    },
                    "responses": {
                        "200": {
                            "content": {
                                "application/json": { "schema": { "$ref": "#/components/schemas/Order" } }
                            }
                        }
                    }
                }
            }
        }
    }"##;

    fn parse_fixture() -> ApiModel {
        Parser
            .parse(FIXTURE.as_bytes())
            .expect("fixture should parse")
    }

    #[test]
    fn maps_fixture_to_expected_model() {
        let model = parse_fixture();
        assert_eq!(model.version, ModelVersion::V1);
        assert_eq!(model.name, "Petstore");
        assert_eq!(model.base_url, "https://api.example.com/v1");
        assert_eq!(model.operation_groups.len(), 3);

        let names: Vec<&str> = model
            .operation_groups
            .iter()
            .map(|g| g.name.as_str())
            .collect();
        assert_eq!(names, vec!["pets", "default", "store"]);

        let pets = &model.operation_groups[0];
        assert_eq!(pets.name, "pets");
        assert_eq!(
            pets.description.as_deref(),
            Some("Everything about your pets")
        );
        let op_names: Vec<&str> = pets.operations.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(op_names, vec!["list-pets", "create-pet"]);
    }

    #[test]
    fn maps_parameters_and_body() {
        let model = parse_fixture();
        let pets = &model.operation_groups[0];

        let list = &pets.operations[0];
        assert_eq!(list.method, HttpMethod::Get);
        assert_eq!(list.path, "/pets");
        assert!(list.path_params.is_empty());
        let query_names: Vec<&str> = list.query_params.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(query_names, vec!["sort", "limit"]);
        let limit = list
            .query_params
            .iter()
            .find(|p| p.name == "limit")
            .unwrap();
        assert!(
            !limit.required,
            "operation-level param overrides path-level"
        );
        assert_eq!(limit.canonical_name, "limit");

        let create = &pets.operations[1];
        let query_names: Vec<&str> = create
            .query_params
            .iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(query_names, vec!["sort", "limit"]);
        let limit = create
            .query_params
            .iter()
            .find(|p| p.name == "limit")
            .unwrap();
        assert!(
            limit.required,
            "path-level required flag applies to other operations"
        );
        let body = create.request_body.as_ref().unwrap();
        assert!(body.required);
        assert_eq!(body.content_type, "application/json");
        assert_eq!(
            body.schema,
            Some(SchemaSpec::Object {
                properties: BTreeMap::new(),
                extra_json: None,
            })
        );
    }

    #[test]
    fn fallback_names_and_default_group() {
        let model = parse_fixture();
        let default = model
            .operation_groups
            .iter()
            .find(|g| g.name == "default")
            .unwrap();
        let get = &default.operations[0];
        assert_eq!(get.name, "get-pets");
        assert_eq!(get.method, HttpMethod::Get);
        assert_eq!(get.path, "/pets/{petId}");
        assert_eq!(get.path_params.len(), 1, "header params are dropped");
        let pet_id = &get.path_params[0];
        assert_eq!(pet_id.name, "petId");
        assert_eq!(pet_id.canonical_name, "pet-id");
        assert!(pet_id.required);
        assert_eq!(pet_id.description.as_deref(), Some("The numeric pet id"));
        assert!(get.request_body.is_none());
    }

    #[test]
    fn disambiguates_colliding_fallback_names() {
        let spec = r#"{
            "openapi": "3.0.0",
            "paths": {
                "/pets": { "get": { "summary": "a" } },
                "/pets/{petId}": { "get": { "summary": "b" } }
            }
        }"#;
        let model = Parser.parse(spec.as_bytes()).unwrap();
        let names: Vec<&str> = model.operation_groups[0]
            .operations
            .iter()
            .map(|c| c.name.as_str())
            .collect();
        assert_eq!(names, vec!["get-pets", "get-pets-2"]);
    }

    #[test]
    fn rejects_openapi_31() {
        let spec = r#"{ "openapi": "3.1.0", "info": {"title":"t","version":"1"}, "paths": {} }"#;
        let err = Parser.parse(spec.as_bytes()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("unsupported OpenAPI version '3.1.0'"), "{msg}");
    }

    #[test]
    fn rejects_swagger_20() {
        let spec = r#"{ "swagger": "2.0", "info": {"title":"t","version":"1"}, "paths": {} }"#;
        let err = Parser.parse(spec.as_bytes()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("Swagger 2.0"), "{msg}");
    }

    #[test]
    fn rejects_malformed_json() {
        let err = Parser.parse(b"not json").unwrap_err();
        let msg = err.to_string();
        assert!(msg.to_lowercase().contains("malformed"), "{msg}");
    }

    #[test]
    fn builds_schema_registry_and_keeps_refs_uninlined() {
        let model = parse_fixture();
        let order = model.schema_by_ref_id("Order").unwrap();
        match order {
            SchemaSpec::Object {
                properties,
                extra_json,
            } => {
                assert_eq!(properties.len(), 3);
                assert_eq!(extra_json.as_deref(), Some(r#"{"required":["id"]}"#));
                match &properties["id"] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""description":"Order id""#)
                                || raw_json.contains(r#""description": "Order id""#)
                        );
                        assert!(
                            raw_json.contains(r#""minimum":1"#)
                                || raw_json.contains(r#""minimum": 1"#)
                        );
                        assert!(
                            raw_json.contains(r#""type":"integer""#)
                                || raw_json.contains(r#""type": "integer""#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
                match &properties["quantity"] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""description":"Number of units""#)
                                || raw_json.contains(r#""description": "Number of units""#)
                        );
                        assert!(
                            raw_json.contains(r#""minimum":1"#)
                                || raw_json.contains(r#""minimum": 1"#)
                        );
                        assert!(
                            raw_json.contains(r#""default":1"#)
                                || raw_json.contains(r#""default": 1"#)
                        );
                        assert!(
                            raw_json.contains(r#""type":"integer""#)
                                || raw_json.contains(r#""type": "integer""#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
                match &properties["petId"] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""type":"integer""#)
                                || raw_json.contains(r#""type": "integer""#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
            }
            other => panic!("expected object schema, got {other:?}"),
        }
    }

    #[test]
    fn request_body_ref_maps_to_typed_registry_entry() {
        let model = parse_fixture();
        let store = model
            .operation_groups
            .iter()
            .find(|g| g.name == "store")
            .unwrap();
        let place_order = store
            .operations
            .iter()
            .find(|o| o.name == "place-order")
            .unwrap();
        let body = place_order.request_body.as_ref().unwrap();
        assert_eq!(
            body.schema,
            Some(SchemaSpec::Ref {
                ref_id: "Order".to_owned()
            })
        );
        assert!(model.schema_by_ref_id("Order").is_some());
    }

    #[test]
    fn captures_response_schemas_per_status_and_content_type() {
        let model = parse_fixture();
        let pets = &model.operation_groups[0];
        let create = pets
            .operations
            .iter()
            .find(|o| o.name == "create-pet")
            .unwrap();
        assert_eq!(create.responses.len(), 1);
        let resp = &create.responses[0];
        assert_eq!(resp.status_code, "200");
        assert_eq!(resp.content_type, "application/json");
        assert_eq!(
            resp.schema,
            Some(SchemaSpec::Object {
                properties: BTreeMap::new(),
                extra_json: None,
            })
        );

        let store = model
            .operation_groups
            .iter()
            .find(|g| g.name == "store")
            .unwrap();
        let place_order = store
            .operations
            .iter()
            .find(|o| o.name == "place-order")
            .unwrap();
        let resp = &place_order.responses[0];
        assert_eq!(resp.status_code, "200");
        assert_eq!(resp.content_type, "application/json");
        assert_eq!(
            resp.schema,
            Some(SchemaSpec::Ref {
                ref_id: "Order".to_owned()
            })
        );
    }

    #[test]
    fn parses_scalar_array_composite_and_unknown_schemas() {
        let spec = r##"{
            "openapi": "3.0.0",
            "components": {
                "schemas": {
                    "Thing": {
                        "type": "object",
                        "properties": {
                            "tags": { "type": "array", "minItems": 1, "items": { "type": "string", "minLength": 2 } },
                            "price": { "type": "number", "minimum": 0.0, "maximum": 999.99 },
                            "active": { "type": "boolean", "default": true }
                        }
                    },
                    "Union": {
                        "description": "A union of string or integer",
                        "oneOf": [
                            { "type": "string" },
                            { "type": "integer", "minimum": 1 }
                        ]
                    },
                    "FreeForm": { "type": "object" },
                    "SelfRef": { "type": "object", "properties": { "next": { "$ref": "#/components/schemas/SelfRef" } } },
                    "External": { "$ref": "https://example.com/schemas.json#/Foo" }
                }
            },
            "paths": {}
        }"##;
        let model = Parser.parse(spec.as_bytes()).unwrap();

        match model.schema_by_ref_id("Thing").unwrap() {
            SchemaSpec::Object {
                properties,
                extra_json,
            } => {
                assert_eq!(*extra_json, None);
                match &properties["tags"] {
                    SchemaSpec::Array { items, extra_json } => {
                        assert_eq!(extra_json.as_deref(), Some(r#"{"minItems":1}"#));
                        match items.as_deref().unwrap() {
                            SchemaSpec::Primitive { raw_json } => {
                                assert!(
                                    raw_json.contains(r#""minLength":2"#)
                                        || raw_json.contains(r#""minLength": 2"#)
                                );
                            }
                            other => panic!("expected primitive, got {other:?}"),
                        }
                    }
                    other => panic!("expected array, got {other:?}"),
                }
                match &properties["price"] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""minimum":0.0"#)
                                || raw_json.contains(r#""minimum": 0.0"#)
                                || raw_json.contains(r#""minimum": 0"#)
                                || raw_json.contains(r#""minimum":0"#)
                        );
                        assert!(
                            raw_json.contains(r#""maximum":999.99"#)
                                || raw_json.contains(r#""maximum": 999.99"#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
                match &properties["active"] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""default":true"#)
                                || raw_json.contains(r#""default": true"#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
            }
            other => panic!("expected object schema, got {other:?}"),
        }
        match model.schema_by_ref_id("Union").unwrap() {
            SchemaSpec::Composite {
                composite_kind,
                schemas,
                extra_json,
            } => {
                assert_eq!(*composite_kind, CompositeKind::OneOf);
                assert_eq!(schemas.len(), 2);
                assert_eq!(
                    extra_json.as_deref(),
                    Some(r#"{"description":"A union of string or integer"}"#)
                );
                match &schemas[1] {
                    SchemaSpec::Primitive { raw_json } => {
                        assert!(
                            raw_json.contains(r#""minimum":1"#)
                                || raw_json.contains(r#""minimum": 1"#)
                        );
                    }
                    other => panic!("expected primitive, got {other:?}"),
                }
            }
            other => panic!("expected composite, got {other:?}"),
        }
        assert_eq!(
            model.schema_by_ref_id("FreeForm").unwrap(),
            &SchemaSpec::Object {
                properties: BTreeMap::new(),
                extra_json: None,
            }
        );
        match model.schema_by_ref_id("SelfRef").unwrap() {
            SchemaSpec::Object {
                properties,
                extra_json,
            } => {
                assert_eq!(*extra_json, None);
                assert_eq!(
                    properties["next"],
                    SchemaSpec::Ref {
                        ref_id: "SelfRef".to_owned()
                    }
                );
            }
            other => panic!("expected object schema, got {other:?}"),
        }
        match model.schema_by_ref_id("External").unwrap() {
            SchemaSpec::Unknown { raw_json } => {
                assert!(raw_json.contains("https://example.com"), "{raw_json}");
            }
            other => panic!("expected unknown schema, got {other:?}"),
        }
    }

    #[test]
    fn untyped_schema_without_properties_is_unknown() {
        let spec = r#"{
            "openapi": "3.0.0",
            "components": { "schemas": { "FreeForm": { "description": "anything" } } },
            "paths": {}
        }"#;
        let model = Parser.parse(spec.as_bytes()).unwrap();
        assert!(matches!(
            model.schema_by_ref_id("FreeForm").unwrap(),
            SchemaSpec::Unknown { .. }
        ));
    }
}
