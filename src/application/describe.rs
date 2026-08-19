//! "Describe" use case producing structured help data (Phase 4).

use crate::domain::model::{
    ApiModel, ApiOperation, ApiOperationGroup, BodySpec, HttpMethod, Param,
};

/// Structured help data for a whole installed API.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelHelp {
    pub name: String,
    pub groups: Vec<GroupHelp>,
}

/// Structured help data for one command group.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupHelp {
    pub name: String,
    pub description: Option<String>,
    pub commands: Vec<CommandHelp>,
}

/// Structured help data for one command (operation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandHelp {
    pub name: String,
    pub summary: Option<String>,
    pub method: HttpMethod,
    pub path: String,
    pub path_params: Vec<ParamHelp>,
    pub query_params: Vec<ParamHelp>,
    pub body: Option<BodyHelp>,
}

/// Structured help data for a path or query parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParamHelp {
    pub name: String,
    pub canonical_name: String,
    pub required: bool,
    pub description: Option<String>,
}

/// Body summary shown in command help.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BodyHelp {
    pub required: bool,
    pub content_type: String,
    pub schema_summary: String,
}

/// Pure "describe" use case: produces structured help data from a model so the
/// CLI layer can render `--help` at every level without re-deriving logic.
pub struct DescribeService;

impl DescribeService {
    /// Produces help data for an entire installed API.
    pub fn describe(model: &ApiModel) -> ModelHelp {
        ModelHelp {
            name: model.name.clone(),
            groups: model
                .operation_groups
                .iter()
                .map(Self::describe_group)
                .collect(),
        }
    }

    /// Produces help data for a single command group.
    pub fn describe_group(group: &ApiOperationGroup) -> GroupHelp {
        GroupHelp {
            name: group.name.clone(),
            description: group.description.clone(),
            commands: group
                .operations
                .iter()
                .map(Self::describe_operation)
                .collect(),
        }
    }

    /// Produces help data for a single command (operation).
    pub fn describe_operation(operation: &ApiOperation) -> CommandHelp {
        CommandHelp {
            name: operation.name.clone(),
            summary: operation.summary.clone(),
            method: operation.method,
            path: operation.path.clone(),
            path_params: operation
                .path_params
                .iter()
                .map(Self::describe_param)
                .collect(),
            query_params: operation
                .query_params
                .iter()
                .map(Self::describe_param)
                .collect(),
            body: operation.request_body.as_ref().map(Self::describe_body),
        }
    }

    fn describe_param(param: &Param) -> ParamHelp {
        ParamHelp {
            name: param.name.clone(),
            canonical_name: param.canonical_name.clone(),
            required: param.required,
            description: param.description.clone(),
        }
    }

    fn describe_body(body: &BodySpec) -> BodyHelp {
        BodyHelp {
            required: body.required,
            content_type: body.content_type.clone(),
            schema_summary: schema_summary(body.schema_json.as_deref()),
        }
    }
}

/// Derives a short human-readable summary of a raw JSON Schema fragment.
fn schema_summary(schema_json: Option<&str>) -> String {
    let Some(json) = schema_json else {
        return "unspecified schema".to_owned();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return "schema".to_owned();
    };
    match value.get("type").and_then(serde_json::Value::as_str) {
        Some(t) => t.to_owned(),
        None if value.get("$ref").is_some() => "reference".to_owned(),
        None if value.get("properties").is_some() => "object".to_owned(),
        None if value.get("items").is_some() => "array".to_owned(),
        None => "schema".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::model::{ModelVersion, Param};

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            operation_groups: vec![
                ApiOperationGroup {
                    name: "pets".to_owned(),
                    description: Some("Everything about your pets".to_owned()),
                    operations: vec![ApiOperation {
                        name: "get-pet".to_owned(),
                        summary: Some("Get a pet by id".to_owned()),
                        method: HttpMethod::Get,
                        path: "/pets/{petId}".to_owned(),
                        path_params: vec![Param {
                            name: "petId".to_owned(),
                            canonical_name: "pet-id".to_owned(),
                            required: true,
                            description: Some("Numeric id of the pet".to_owned()),
                        }],
                        query_params: vec![Param {
                            name: "status".to_owned(),
                            canonical_name: "status".to_owned(),
                            required: false,
                            description: None,
                        }],
                        request_body: Some(BodySpec {
                            required: true,
                            content_type: "application/json".to_owned(),
                            schema_json: Some(r#"{"type":"object"}"#.to_owned()),
                        }),
                    }],
                },
                ApiOperationGroup {
                    name: "store".to_owned(),
                    description: None,
                    operations: vec![],
                },
            ],
        }
    }

    #[test]
    fn describe_produces_groups_commands_and_params() {
        let help = DescribeService::describe(&sample_model());
        assert_eq!(help.name, "pets");
        assert_eq!(help.groups.len(), 2);
        let pets = &help.groups[0];
        assert_eq!(pets.name, "pets");
        assert_eq!(
            pets.description.as_deref(),
            Some("Everything about your pets")
        );
        let command = &pets.commands[0];
        assert_eq!(command.name, "get-pet");
        assert_eq!(command.summary.as_deref(), Some("Get a pet by id"));
        assert_eq!(command.method, HttpMethod::Get);
        assert_eq!(command.path, "/pets/{petId}");
        assert_eq!(
            command.path_params[0].description.as_deref(),
            Some("Numeric id of the pet")
        );
        assert!(command.path_params[0].required);
        assert!(!command.query_params[0].required);
    }

    #[test]
    fn describe_produces_body_help() {
        let help = DescribeService::describe(&sample_model());
        let body = help.groups[0].commands[0].body.as_ref().unwrap();
        assert!(body.required);
        assert_eq!(body.content_type, "application/json");
        assert_eq!(body.schema_summary, "object");
    }

    #[test]
    fn schema_summary_variants() {
        assert_eq!(schema_summary(Some(r#"{"type":"array"}"#)), "array");
        assert_eq!(
            schema_summary(Some(r##"{"$ref":"#/components/schemas/Order"}"##)),
            "reference"
        );
        assert_eq!(
            schema_summary(Some(r#"{"properties":{"name":{"type":"string"}}}"#)),
            "object"
        );
        assert_eq!(schema_summary(Some("not json")), "schema");
        assert_eq!(schema_summary(None), "unspecified schema");
    }
}
