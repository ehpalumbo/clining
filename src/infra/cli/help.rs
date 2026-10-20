//! Plain-text help rendering for API operation commands.
//!
//! clap renders the static skeleton (about, usage, options) and exposes this
//! module's output through `after_help`. The walkthroughs here are recursive
//! and data-driven, which clap's `help_template` placeholders cannot express,
//! so they are built as plain text and kept independent of the CLI parser for
//! direct unit testing.

use std::fmt::Write;

use crate::domain::model::{
    ApiModel, ApiOperation, BodySpec, CompositeKind, Param, ResponseSpec, SchemaSpec,
};
use crate::infra::openapi::parser::parse_schema_value;

/// Renders the long-form input/output documentation shown under a command's
/// `--help`: the request line, request body walkthrough, and response schemas.
/// Parameter value shapes are attached to the generated clap arguments.
pub(crate) fn command_after_help(model: &ApiModel, operation: &ApiOperation) -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "Request: {} {}",
        operation.method.as_str(),
        operation.path
    );

    let _ = writeln!(text, "\nRequest body");
    match &operation.request_body {
        Some(body) => render_request_body(model, body, &mut text),
        None => {
            let _ = writeln!(text, "  none");
        }
    }

    let _ = writeln!(text, "\nResponses");
    if operation.responses.is_empty() {
        let _ = writeln!(text, "  none documented");
    } else {
        for response in &operation.responses {
            render_response(model, response, &mut text);
        }
    }

    text
}

/// Help text for a single clap argument: description, requiredness, and the
/// spec-provided value shape (type, enum, examples, constraints). Structured
/// schemas append an indented field walkthrough so object and array parameters
/// are documented, not just labeled with their top-level type.
pub(crate) fn param_help_text(model: &ApiModel, param: &Param) -> String {
    let mut text = param.description.clone().unwrap_or_default();
    if !text.is_empty() {
        text.push(' ');
    }
    text.push_str(if param.required {
        "(required)"
    } else {
        "(optional)"
    });
    if let Some(schema) = &param.schema {
        let details = parameter_details(model, schema);
        if !details.is_empty() {
            let _ = write!(text, " ({details})");
        }
        if let Some(walkthrough) = param_walkthrough(model, schema) {
            text.push('\n');
            text.push_str(&walkthrough);
        }
    }
    text
}

/// Renders the children of a structured parameter schema at one indent level.
/// Primitive parameters yield `None`, so their help stays a single line.
fn param_walkthrough(model: &ApiModel, schema: &SchemaSpec) -> Option<String> {
    let mut seen = Vec::new();
    let mut nested = String::new();
    match schema {
        SchemaSpec::Ref { ref_id } => match resolve_ref(model, ref_id, &seen) {
            RefResolution::Target {
                chain,
                schema: target,
            } => {
                seen.extend(chain);
                render_schema_children(model, target, 1, &mut seen, &mut nested);
            }
            RefResolution::Recursive | RefResolution::Unresolved => {}
        },
        _ => render_schema_children(model, schema, 1, &mut seen, &mut nested),
    }
    let nested = nested.trim_end_matches('\n');
    if nested.is_empty() {
        None
    } else {
        Some(nested.to_owned())
    }
}

fn parameter_details(model: &ApiModel, schema: &SchemaSpec) -> String {
    let (type_label, meta) = resolved_meta(model, schema);
    let mut parts = Vec::new();
    if !type_label.is_empty() {
        parts.push(format!("type: {type_label}"));
    }
    parts.extend(meta.details());
    parts.join("; ")
}

fn render_request_body(model: &ApiModel, body: &BodySpec, text: &mut String) {
    let _ = writeln!(
        text,
        "  {} ({})",
        body.content_type,
        if body.required {
            "required"
        } else {
            "optional"
        }
    );
    match &body.schema {
        Some(schema) => render_schema_entry(model, schema, "body", 2, false, &mut Vec::new(), text),
        None => {
            let _ = writeln!(text, "    schema not documented");
        }
    }
}

fn render_response(model: &ApiModel, response: &ResponseSpec, text: &mut String) {
    match &response.content_type {
        Some(content_type) => {
            let _ = writeln!(text, "  {} {content_type}", response.status_code);
        }
        None => {
            let _ = writeln!(text, "  {}", response.status_code);
        }
    }
    match &response.schema {
        Some(schema) => render_schema_entry(model, schema, "body", 2, false, &mut Vec::new(), text),
        None if response.content_type.is_none() => {}
        None => {
            let _ = writeln!(text, "    schema not documented");
        }
    }
}

/// Outcome of following a local `$ref`, including alias components.
enum RefResolution<'a> {
    /// The final non-ref schema, with the chain of ref ids followed to reach it.
    Target {
        chain: Vec<String>,
        schema: &'a SchemaSpec,
    },
    /// The chain loops back to a ref that is already being rendered.
    Recursive,
    /// A ref in the chain is not present in the registry.
    Unresolved,
}

/// Follows local `$ref` aliases until a concrete schema is reached.
///
/// A component may itself be a `$ref` (for example `Alias: {$ref: Pet}`), so
/// resolving one level is not enough to find the shape or children. `seen` is
/// the stack of refs currently being expanded, which terminates cycles.
fn resolve_ref<'a>(model: &'a ApiModel, ref_id: &str, seen: &[String]) -> RefResolution<'a> {
    let mut chain = vec![ref_id.to_owned()];
    if seen.iter().any(|id| id == ref_id) {
        return RefResolution::Recursive;
    }
    let mut current = model.schema_by_ref_id(ref_id);
    while let Some(SchemaSpec::Ref { ref_id: next }) = current {
        if seen.iter().any(|id| id == next) || chain.iter().any(|id| id == next) {
            return RefResolution::Recursive;
        }
        chain.push(next.clone());
        current = model.schema_by_ref_id(next);
    }
    match current {
        Some(schema) => RefResolution::Target { chain, schema },
        None => RefResolution::Unresolved,
    }
}

/// Renders one schema node (a property, body, response, array item, or
/// composite branch) with its label, resolved shape, metadata, and children.
/// Nested structures use JSON-like delimiters (braces for objects/composites,
/// brackets for arrays), so the outline stays unambiguous even when
/// descriptions contain punctuation.
fn render_schema_entry(
    model: &ApiModel,
    schema: &SchemaSpec,
    label: &str,
    depth: usize,
    required: bool,
    seen: &mut Vec<String>,
    text: &mut String,
) {
    let indent = "  ".repeat(depth);
    match schema {
        SchemaSpec::Ref { ref_id } => match resolve_ref(model, ref_id, seen) {
            RefResolution::Recursive => {
                let item = format!("{ref_id} (recursive)");
                let _ = writeln!(text, "{indent}{}", schema_head_line(label, &item, required));
            }
            RefResolution::Unresolved => {
                let item = format!("{ref_id} (unresolved reference)");
                let _ = writeln!(text, "{indent}{}", schema_head_line(label, &item, required));
            }
            RefResolution::Target {
                chain,
                schema: target,
            } => {
                let type_label = ref_type_label(label, ref_id, &chain, target);
                let mut line = schema_line(label, &type_label, required, meta_from_schema(target));
                let mut nested_seen = seen.clone();
                nested_seen.extend(chain.iter().cloned());
                let opens = schema_has_children(model, target, &nested_seen);
                let (open, close) = delimiters(target);
                if opens {
                    line.push(' ');
                    line.push_str(open);
                }
                let _ = writeln!(text, "{indent}{line}");
                if opens {
                    let base = seen.len();
                    seen.extend(chain.iter().cloned());
                    render_schema_children(model, target, depth + 1, seen, text);
                    seen.truncate(base);
                    let _ = writeln!(text, "{indent}{close}");
                }
            }
        },
        _ => {
            let mut line = schema_line(
                label,
                &type_display(schema),
                required,
                meta_from_schema(schema),
            );
            let opens = schema_has_children(model, schema, seen);
            let (open, close) = delimiters(schema);
            if opens {
                line.push(' ');
                line.push_str(open);
            }
            let _ = writeln!(text, "{indent}{line}");
            if opens {
                render_schema_children(model, schema, depth + 1, seen, text);
                let _ = writeln!(text, "{indent}{close}");
            }
        }
    }
}

/// Array bodies use square brackets; all other containers use braces.
fn delimiters(schema: &SchemaSpec) -> (&'static str, &'static str) {
    match schema {
        SchemaSpec::Array { .. } => ("[", "]"),
        _ => ("{", "}"),
    }
}

/// Whether a node expands into nested entries (and therefore needs a bracketed
/// or braced body).
fn schema_has_children(model: &ApiModel, schema: &SchemaSpec, seen: &[String]) -> bool {
    match schema {
        SchemaSpec::Ref { ref_id } => {
            if seen.iter().any(|id| id == ref_id) {
                return false;
            }
            let mut nested_seen = seen.to_vec();
            nested_seen.push(ref_id.clone());
            model
                .schema_by_ref_id(ref_id)
                .is_some_and(|target| schema_has_children(model, target, &nested_seen))
        }
        SchemaSpec::Object { properties, .. } => !properties.is_empty(),
        SchemaSpec::Array { items, .. } => items.as_deref().is_some_and(needs_item_line),
        SchemaSpec::Composite { schemas, .. } => !schemas.is_empty(),
        SchemaSpec::Primitive { .. } | SchemaSpec::Unknown { .. } => false,
    }
}

fn render_schema_children(
    model: &ApiModel,
    schema: &SchemaSpec,
    depth: usize,
    seen: &mut Vec<String>,
    text: &mut String,
) {
    match schema {
        SchemaSpec::Object {
            properties,
            extra_json,
        } => {
            let required = required_names(extra_json.as_deref());
            for (name, property) in properties {
                render_schema_entry(
                    model,
                    property,
                    name,
                    depth,
                    required.iter().any(|r| r == name),
                    seen,
                    text,
                );
            }
        }
        SchemaSpec::Array { items, .. } => {
            if let Some(items) = items.as_deref()
                && needs_item_line(items)
            {
                render_schema_entry(model, items, "", depth, false, seen, text);
            }
        }
        SchemaSpec::Composite {
            composite_kind,
            schemas,
            extra_json,
        } => {
            let branch_label = match composite_kind {
                CompositeKind::AllOf => "part",
                CompositeKind::OneOf | CompositeKind::AnyOf => "option",
            };
            for (index, branch) in schemas.iter().enumerate() {
                render_schema_entry(
                    model,
                    branch,
                    &format!("{branch_label} {}", index + 1),
                    depth,
                    false,
                    seen,
                    text,
                );
            }
            render_composite_siblings(model, extra_json.as_deref(), depth, seen, text);
        }
        // Defensive: callers normally resolve refs before rendering children,
        // but following refs here keeps alias components rendering correctly.
        SchemaSpec::Ref { .. } => {
            render_schema_entry(model, schema, "", depth, false, seen, text);
        }
        _ => {}
    }
}

/// Renders `properties`/`required` declared alongside a composite keyword
/// (`allOf`/`oneOf`/`anyOf`).
///
/// The parser preserves those siblings as unmodeled metadata in `extra_json`,
/// so without this they never appear in help even though they constrain the
/// instance.
fn render_composite_siblings(
    model: &ApiModel,
    extra_json: Option<&str>,
    depth: usize,
    seen: &mut Vec<String>,
    text: &mut String,
) {
    let Some(raw) = extra_json else {
        return;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return;
    };
    let Some(properties) = value
        .get("properties")
        .and_then(serde_json::Value::as_object)
    else {
        return;
    };
    let required = required_names(Some(raw));
    for (name, property) in properties {
        let schema = parse_schema_value(property);
        render_schema_entry(
            model,
            &schema,
            name,
            depth,
            required.iter().any(|r| r == name),
            seen,
            text,
        );
    }
}

/// Structured item schemas need their own line; primitive items need one only
/// when they carry metadata worth showing (enum, format, constraints, …).
fn needs_item_line(schema: &SchemaSpec) -> bool {
    if has_structure(schema) {
        return true;
    }
    !meta_from_schema(schema).details().is_empty()
}

fn has_structure(schema: &SchemaSpec) -> bool {
    matches!(
        schema,
        SchemaSpec::Object { .. }
            | SchemaSpec::Array { .. }
            | SchemaSpec::Composite { .. }
            | SchemaSpec::Ref { .. }
    )
}

/// Builds a visible schema entry: `label: type`, required marker, description,
/// and compact metadata. Array items have no label because the enclosing
/// brackets already identify them.
fn schema_line(label: &str, type_label: &str, required: bool, meta: SchemaMeta) -> String {
    let mut line = schema_head_line(label, type_label, required);
    if let Some(description) = &meta.description {
        let _ = write!(line, ": {description}");
    }
    let details = meta.details();
    if !details.is_empty() {
        let _ = write!(line, " ({})", details.join("; "));
    }
    line
}

/// Builds `label: type` with an optional required marker, without metadata.
fn schema_head_line(label: &str, type_label: &str, required: bool) -> String {
    let mut line = schema_head(label, type_label);
    if required {
        line.push_str(" (required)");
    }
    line
}

fn schema_head(label: &str, type_label: &str) -> String {
    if label.is_empty() {
        type_label.to_owned()
    } else {
        format!("{label}: {type_label}")
    }
}

fn type_display(schema: &SchemaSpec) -> String {
    match schema {
        SchemaSpec::Ref { ref_id } => ref_id.clone(),
        SchemaSpec::Object { .. } => "object".to_owned(),
        SchemaSpec::Array { items, .. } => match items.as_deref() {
            Some(items) => format!("array<{}>", type_display(items)),
            None => "array".to_owned(),
        },
        SchemaSpec::Primitive { raw_json } => {
            let meta = meta_from_json_str(raw_json);
            meta.type_name.unwrap_or_else(|| "unknown".to_owned())
        }
        SchemaSpec::Composite { composite_kind, .. } => composite_kind.keyword().to_owned(),
        SchemaSpec::Unknown { .. } => "unknown".to_owned(),
    }
}

/// Builds the `label: type` type portion for a resolved ref.
///
/// A direct ref renders as `Order (object)`. Alias chains keep the intermediate
/// component names (`Alias (Pet, object)`) so the resolved shape stays
/// traceable back to the spec.
fn ref_type_label(label: &str, ref_id: &str, chain: &[String], target: &SchemaSpec) -> String {
    let target_type = type_display(target);
    if label == ref_id {
        return target_type;
    }
    if chain.len() <= 1 {
        return format!("{ref_id} ({target_type})");
    }
    let mut names: Vec<String> = chain[1..].to_vec();
    names.push(target_type);
    format!("{ref_id} ({})", names.join(", "))
}

/// Resolves a schema for metadata display, following local ref aliases so a
/// parameter declared as `$ref: Status` still shows the target's enum and type.
fn resolved_meta(model: &ApiModel, schema: &SchemaSpec) -> (String, SchemaMeta) {
    match schema {
        SchemaSpec::Ref { ref_id } => match resolve_ref(model, ref_id, &[]) {
            RefResolution::Target {
                chain,
                schema: target,
            } => (
                ref_type_label("", ref_id, &chain, target),
                meta_from_schema(target),
            ),
            RefResolution::Recursive => (format!("{ref_id} (recursive)"), SchemaMeta::default()),
            RefResolution::Unresolved => (format!("{ref_id} (unresolved)"), SchemaMeta::default()),
        },
        _ => (type_display(schema), meta_from_schema(schema)),
    }
}

#[derive(Debug, Default, PartialEq)]
struct SchemaMeta {
    type_name: Option<String>,
    format: Option<String>,
    description: Option<String>,
    enum_values: Vec<serde_json::Value>,
    example: Option<serde_json::Value>,
    examples: Vec<serde_json::Value>,
    default: Option<serde_json::Value>,
    constraints: Vec<(String, serde_json::Value)>,
    nullable: bool,
}

impl SchemaMeta {
    fn details(&self) -> Vec<String> {
        let mut parts = Vec::new();
        if let Some(format) = &self.format {
            parts.push(format!("format: {format}"));
        }
        if !self.enum_values.is_empty() {
            parts.push(format!("enum: {}", join_values(&self.enum_values)));
        }
        for (key, value) in &self.constraints {
            parts.push(format!("{key}: {}", compact(value)));
        }
        if let Some(default) = &self.default {
            parts.push(format!("default: {}", compact(default)));
        }
        if let Some(example) = &self.example {
            parts.push(format!("example: {}", compact(example)));
        }
        if !self.examples.is_empty() {
            parts.push(format!("examples: {}", join_values(&self.examples)));
        }
        if self.nullable {
            parts.push("nullable".to_owned());
        }
        parts
    }
}

fn join_values(values: &[serde_json::Value]) -> String {
    values.iter().map(compact).collect::<Vec<_>>().join(", ")
}

fn compact(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn meta_from_schema(schema: &SchemaSpec) -> SchemaMeta {
    match schema {
        SchemaSpec::Object { extra_json, .. } => {
            let mut meta = meta_from_extra(extra_json.as_deref());
            meta.type_name = Some("object".to_owned());
            meta
        }
        SchemaSpec::Array { extra_json, .. } => {
            let mut meta = meta_from_extra(extra_json.as_deref());
            meta.type_name = Some("array".to_owned());
            meta
        }
        SchemaSpec::Composite {
            composite_kind,
            extra_json,
            ..
        } => {
            let mut meta = meta_from_extra(extra_json.as_deref());
            meta.type_name = Some(composite_kind.keyword().to_owned());
            meta
        }
        SchemaSpec::Primitive { raw_json } | SchemaSpec::Unknown { raw_json } => {
            meta_from_json_str(raw_json)
        }
        SchemaSpec::Ref { ref_id } => SchemaMeta {
            type_name: Some(ref_id.clone()),
            ..SchemaMeta::default()
        },
    }
}

fn meta_from_extra(extra_json: Option<&str>) -> SchemaMeta {
    extra_json.map(meta_from_json_str).unwrap_or_default()
}

fn meta_from_json_str(raw_json: &str) -> SchemaMeta {
    let value: serde_json::Value = match serde_json::from_str(raw_json) {
        Ok(value) => value,
        Err(_) => return SchemaMeta::default(),
    };
    let object = match value.as_object() {
        Some(object) => object,
        None => return SchemaMeta::default(),
    };
    let constraint_keys = [
        "minimum",
        "maximum",
        "exclusiveMinimum",
        "exclusiveMaximum",
        "multipleOf",
        "minLength",
        "maxLength",
        "pattern",
        "minItems",
        "maxItems",
        "uniqueItems",
        "minProperties",
        "maxProperties",
    ];
    let enum_values: Vec<serde_json::Value> = object
        .get("enum")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let type_name = object
        .get("type")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned)
        .or_else(|| infer_enum_type(&enum_values));
    SchemaMeta {
        type_name,
        format: object
            .get("format")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        description: object
            .get("description")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        enum_values,
        example: object.get("example").cloned(),
        examples: object
            .get("examples")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default(),
        default: object.get("default").cloned(),
        constraints: constraint_keys
            .iter()
            .filter_map(|key| {
                object
                    .get(*key)
                    .map(|value| ((*key).to_owned(), value.clone()))
            })
            .collect(),
        nullable: object
            .get("nullable")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false),
    }
}

/// Infers a type for an enum-only schema (`{"enum": [...]}`).
fn infer_enum_type(values: &[serde_json::Value]) -> Option<String> {
    let first = values.first()?;
    let inferred = match first {
        serde_json::Value::String(_) => "string",
        serde_json::Value::Bool(_) => "boolean",
        serde_json::Value::Number(number) if number.is_i64() || number.is_u64() => "integer",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
        serde_json::Value::Null => "null",
    };
    Some(inferred.to_owned())
}

fn required_names(extra_json: Option<&str>) -> Vec<String> {
    let Some(raw) = extra_json else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return Vec::new();
    };
    value
        .get("required")
        .and_then(serde_json::Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use crate::domain::model::{HttpMethod, ModelVersion};

    fn primitive(raw_json: &str) -> SchemaSpec {
        SchemaSpec::Primitive {
            raw_json: raw_json.to_owned(),
        }
    }

    fn object(properties: BTreeMap<String, SchemaSpec>, extra_json: Option<String>) -> SchemaSpec {
        SchemaSpec::Object {
            properties,
            extra_json,
        }
    }

    fn model_with_registry(registry: BTreeMap<String, SchemaSpec>) -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: registry,
            auth: None,
            operation_groups: vec![],
        }
    }

    fn operation(
        path_params: Vec<Param>,
        query_params: Vec<Param>,
        request_body: Option<BodySpec>,
        responses: Vec<ResponseSpec>,
    ) -> ApiOperation {
        ApiOperation {
            name: "place-order".to_owned(),
            summary: Some("Place an order".to_owned()),
            method: HttpMethod::Post,
            path: "/store/orders".to_owned(),
            path_params,
            query_params,
            request_body,
            responses,
        }
    }

    fn order_registry() -> BTreeMap<String, SchemaSpec> {
        BTreeMap::from([(
            "Order".to_owned(),
            object(
                BTreeMap::from([
                    (
                        "id".to_owned(),
                        primitive(r#"{"type":"integer","description":"Order id","minimum":1}"#),
                    ),
                    (
                        "quantity".to_owned(),
                        primitive(
                            r#"{"type":"integer","description":"Number of units","minimum":1,"default":1}"#,
                        ),
                    ),
                    (
                        "status".to_owned(),
                        primitive(r#"{"type":"string","enum":["open","closed"],"example":"open"}"#),
                    ),
                    (
                        "tags".to_owned(),
                        SchemaSpec::Array {
                            items: Some(Box::new(primitive(r#"{"type":"string"}"#))),
                            extra_json: None,
                        },
                    ),
                    (
                        "owner".to_owned(),
                        object(
                            BTreeMap::from([(
                                "name".to_owned(),
                                primitive(r#"{"type":"string","description":"Owner name"}"#),
                            )]),
                            Some(r#"{"required":["name"]}"#.to_owned()),
                        ),
                    ),
                ]),
                Some(r#"{"required":["id","status"]}"#.to_owned()),
            ),
        )])
    }

    fn place_order_body() -> BodySpec {
        BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Order".to_owned(),
            }),
        }
    }

    #[test]
    fn renders_request_line_and_ref_body_walkthrough() {
        let model = model_with_registry(order_registry());
        let help = command_after_help(
            &model,
            &operation(vec![], vec![], Some(place_order_body()), vec![]),
        );

        assert!(help.contains("Request: POST /store/orders"), "{help}");
        assert!(
            help.contains("Request body\n  application/json (required)"),
            "{help}"
        );
        assert!(help.contains("body: Order (object) {"), "{help}");
        assert!(
            help.contains("id: integer (required): Order id (minimum: 1)"),
            "{help}"
        );
        assert!(
            help.contains("quantity: integer: Number of units (minimum: 1; default: 1)"),
            "{help}"
        );
        assert!(
            help.contains(r#"status: string (required) (enum: "open", "closed"; example: "open")"#),
            "{help}"
        );
        assert!(help.contains("tags: array<string>"), "{help}");
        assert!(!help.contains("$ref"), "{help}");
    }

    #[test]
    fn renders_nested_objects_and_array_items() {
        let model = model_with_registry(order_registry());
        let help = command_after_help(
            &model,
            &operation(vec![], vec![], Some(place_order_body()), vec![]),
        );

        assert!(help.contains("owner: object {"), "{help}");
        assert!(
            help.contains("name: string (required): Owner name"),
            "{help}"
        );
        assert!(!help.contains(r#"{"type":"string"}"#), "{help}");
    }

    #[test]
    fn renders_arrays_of_refs_with_component_name() {
        let registry = BTreeMap::from([
            ("Order".to_owned(), primitive(r#"{"type":"object"}"#)),
            (
                "Cart".to_owned(),
                object(
                    BTreeMap::from([(
                        "orders".to_owned(),
                        SchemaSpec::Array {
                            items: Some(Box::new(SchemaSpec::Ref {
                                ref_id: "Order".to_owned(),
                            })),
                            extra_json: None,
                        },
                    )]),
                    None,
                ),
            ),
        ]);
        let model = model_with_registry(registry);
        let body = BodySpec {
            required: false,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Cart".to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));

        assert!(help.contains("body: Cart (object) {"), "{help}");
        assert!(help.contains("orders: array<Order> ["), "{help}");
        assert!(help.contains("Order (object)"), "{help}");
        assert!(!help.contains("items:"), "{help}");
    }

    #[test]
    fn marks_recursive_refs_without_raw_refs() {
        let registry = BTreeMap::from([(
            "Node".to_owned(),
            object(
                BTreeMap::from([(
                    "next".to_owned(),
                    SchemaSpec::Ref {
                        ref_id: "Node".to_owned(),
                    },
                )]),
                Some(r#"{"required":["next"]}"#.to_owned()),
            ),
        )]);
        let model = model_with_registry(registry);
        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Node".to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));

        assert!(help.contains("body: Node (object) {"), "{help}");
        assert!(help.contains("next: Node (recursive) (required)"), "{help}");
        assert!(!help.contains("$ref"), "{help}");
    }

    #[test]
    fn follows_ref_alias_components() {
        let registry = BTreeMap::from([
            (
                "Pet".to_owned(),
                object(
                    BTreeMap::from([(
                        "id".to_owned(),
                        primitive(r#"{"type":"integer","description":"Pet id"}"#),
                    )]),
                    Some(r#"{"required":["id"]}"#.to_owned()),
                ),
            ),
            (
                "Alias".to_owned(),
                SchemaSpec::Ref {
                    ref_id: "Pet".to_owned(),
                },
            ),
            (
                "Status".to_owned(),
                primitive(r#"{"type":"string","enum":["open","closed"]}"#),
            ),
            (
                "AliasStatus".to_owned(),
                SchemaSpec::Ref {
                    ref_id: "Status".to_owned(),
                },
            ),
        ]);
        let model = model_with_registry(registry);
        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Alias".to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));

        assert!(help.contains("body: Alias (Pet, object) {"), "{help}");
        assert!(help.contains("id: integer (required): Pet id"), "{help}");

        let param = Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
            schema: Some(SchemaSpec::Ref {
                ref_id: "AliasStatus".to_owned(),
            }),
        };
        assert_eq!(
            param_help_text(&model, &param),
            r#"(optional) (type: AliasStatus (Status, string); enum: "open", "closed")"#
        );
    }

    #[test]
    fn renders_composite_branches() {
        let schema = SchemaSpec::Composite {
            composite_kind: CompositeKind::OneOf,
            schemas: vec![
                primitive(r#"{"type":"string"}"#),
                SchemaSpec::Ref {
                    ref_id: "Order".to_owned(),
                },
            ],
            extra_json: None,
        };
        let model = model_with_registry(order_registry());
        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(schema),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));

        assert!(help.contains("body: oneOf {"), "{help}");
        assert!(help.contains("option 1: string"), "{help}");
        assert!(help.contains("option 2: Order (object) {"), "{help}");
    }

    #[test]
    fn renders_properties_declared_alongside_composites() {
        let registry = BTreeMap::from([
            (
                "Base".to_owned(),
                object(
                    BTreeMap::from([("id".to_owned(), primitive(r#"{"type":"integer"}"#))]),
                    Some(r#"{"required":["id"]}"#.to_owned()),
                ),
            ),
            (
                "Extended".to_owned(),
                SchemaSpec::Composite {
                    composite_kind: CompositeKind::AllOf,
                    schemas: vec![SchemaSpec::Ref {
                        ref_id: "Base".to_owned(),
                    }],
                    extra_json: Some(
                        r#"{"required":["name"],"properties":{"name":{"type":"string","description":"Display name"}}}"#
                            .to_owned(),
                    ),
                },
            ),
        ]);
        let model = model_with_registry(registry);
        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Extended".to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));

        assert!(help.contains("body: Extended (allOf) {"), "{help}");
        assert!(help.contains("part 1: Base (object) {"), "{help}");
        assert!(
            help.contains("name: string (required): Display name"),
            "{help}"
        );
    }

    #[test]
    fn renders_responses_with_status_content_type_and_schema() {
        let registry = BTreeMap::from([
            (
                "Order".to_owned(),
                object(
                    BTreeMap::from([(
                        "id".to_owned(),
                        primitive(r#"{"type":"integer","minimum":1}"#),
                    )]),
                    Some(r#"{"required":["id"]}"#.to_owned()),
                ),
            ),
            (
                "Error".to_owned(),
                object(
                    BTreeMap::from([("message".to_owned(), primitive(r#"{"type":"string"}"#))]),
                    Some(r#"{"required":["message"]}"#.to_owned()),
                ),
            ),
        ]);
        let model = model_with_registry(registry);
        let responses = vec![
            ResponseSpec {
                status_code: "200".to_owned(),
                content_type: Some("application/json".to_owned()),
                schema: Some(SchemaSpec::Ref {
                    ref_id: "Order".to_owned(),
                }),
            },
            ResponseSpec {
                status_code: "404".to_owned(),
                content_type: Some("application/json".to_owned()),
                schema: Some(SchemaSpec::Ref {
                    ref_id: "Error".to_owned(),
                }),
            },
            ResponseSpec {
                status_code: "204".to_owned(),
                content_type: None,
                schema: None,
            },
        ];
        let help = command_after_help(&model, &operation(vec![], vec![], None, responses));

        assert!(help.contains("Responses"), "{help}");
        assert!(help.contains("200 application/json"), "{help}");
        assert!(help.contains("body: Order (object) {"), "{help}");
        assert!(
            help.contains("id: integer (required) (minimum: 1)"),
            "{help}"
        );
        assert!(help.contains("404 application/json"), "{help}");
        assert!(help.contains("body: Error (object) {"), "{help}");
        assert!(help.contains("message: string (required)"), "{help}");
        assert!(help.contains("204"), "{help}");
        assert!(!help.contains("$ref"), "{help}");
    }

    #[test]
    fn documents_missing_and_unresolved_schemas_without_raw_json() {
        let model = model_with_registry(BTreeMap::new());
        let body = BodySpec {
            required: false,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Ref {
                ref_id: "Missing".to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));
        assert!(
            help.contains("body: Missing (unresolved reference)"),
            "{help}"
        );
        assert!(!help.contains("$ref"), "{help}");

        let no_schema = BodySpec {
            required: false,
            content_type: "application/json".to_owned(),
            schema: None,
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(no_schema), vec![]));
        assert!(help.contains("schema not documented"), "{help}");

        let body_with_missing = BodySpec {
            required: false,
            content_type: "application/json".to_owned(),
            schema: Some(object(
                BTreeMap::from([(
                    "missing".to_owned(),
                    SchemaSpec::Ref {
                        ref_id: "Missing".to_owned(),
                    },
                )]),
                Some(r#"{"required":["missing"]}"#.to_owned()),
            )),
        };
        let help = command_after_help(
            &model,
            &operation(vec![], vec![], Some(body_with_missing), vec![]),
        );
        assert!(
            help.contains("missing: Missing (unresolved reference) (required)"),
            "{help}"
        );
    }

    #[test]
    fn param_help_renders_schema_metadata() {
        let model = model_with_registry(BTreeMap::new());
        let param = Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: true,
            description: Some("Filter by status".to_owned()),
            schema: Some(primitive(
                r#"{"type":"string","enum":["open","closed"],"example":"open","minLength":2}"#,
            )),
        };
        let help = param_help_text(&model, &param);
        assert_eq!(
            help,
            r#"Filter by status (required) (type: string; enum: "open", "closed"; minLength: 2; example: "open")"#
        );
    }

    #[test]
    fn param_help_marks_optional_and_resolves_refs() {
        let registry = BTreeMap::from([(
            "Status".to_owned(),
            primitive(r#"{"type":"string","enum":["open","closed"]}"#),
        )]);
        let model = model_with_registry(registry);
        let optional = Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
            schema: Some(SchemaSpec::Ref {
                ref_id: "Status".to_owned(),
            }),
        };
        assert_eq!(
            param_help_text(&model, &optional),
            r#"(optional) (type: Status (string); enum: "open", "closed")"#
        );
    }

    #[test]
    fn param_help_walks_object_and_array_item_shapes() {
        let registry = BTreeMap::from([(
            "Filter".to_owned(),
            object(
                BTreeMap::from([
                    (
                        "kind".to_owned(),
                        primitive(r#"{"type":"string","enum":["a","b"]}"#),
                    ),
                    (
                        "limit".to_owned(),
                        primitive(r#"{"type":"integer","minimum":1}"#),
                    ),
                ]),
                Some(r#"{"required":["kind"]}"#.to_owned()),
            ),
        )]);
        let model = model_with_registry(registry);
        let object_param = Param {
            name: "filter".to_owned(),
            canonical_name: "filter".to_owned(),
            required: false,
            description: None,
            schema: Some(SchemaSpec::Ref {
                ref_id: "Filter".to_owned(),
            }),
        };
        assert_eq!(
            param_help_text(&model, &object_param),
            "(optional) (type: Filter (object))\n  kind: string (required) (enum: \"a\", \"b\")\n  limit: integer (minimum: 1)"
        );

        let array_param = Param {
            name: "tags".to_owned(),
            canonical_name: "tags".to_owned(),
            required: false,
            description: None,
            schema: Some(SchemaSpec::Array {
                items: Some(Box::new(primitive(r#"{"type":"string","enum":["x","y"]}"#))),
                extra_json: None,
            }),
        };
        assert_eq!(
            param_help_text(&model, &array_param),
            "(optional) (type: array<string>)\n  string (enum: \"x\", \"y\")"
        );
    }

    #[test]
    fn infers_enum_type_and_renders_array_item_metadata() {
        let model = model_with_registry(BTreeMap::new());
        let enum_only = Param {
            name: "sort".to_owned(),
            canonical_name: "sort".to_owned(),
            required: false,
            description: None,
            schema: Some(primitive(r#"{"enum":["asc","desc"]}"#)),
        };
        assert_eq!(
            param_help_text(&model, &enum_only),
            r#"(optional) (type: string; enum: "asc", "desc")"#
        );

        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(object(
                BTreeMap::from([(
                    "tags".to_owned(),
                    SchemaSpec::Array {
                        items: Some(Box::new(primitive(r#"{"type":"string","enum":["a","b"]}"#))),
                        extra_json: None,
                    },
                )]),
                None,
            )),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));
        assert!(help.contains("tags: array<string> ["), "{help}");
        assert!(help.contains(r#"string (enum: "a", "b")"#), "{help}");
        assert!(!help.contains("items:"), "{help}");
    }

    #[test]
    fn unknown_schema_renders_without_raw_json() {
        let model = model_with_registry(BTreeMap::new());
        let body = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema: Some(SchemaSpec::Unknown {
                raw_json: r#"{"x-extension":true}"#.to_owned(),
            }),
        };
        let help = command_after_help(&model, &operation(vec![], vec![], Some(body), vec![]));
        assert!(help.contains("body: unknown"), "{help}");
        assert!(!help.contains("x-extension"), "{help}");
    }
}
