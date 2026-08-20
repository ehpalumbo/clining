//! Persisted API model entities (Phase 2).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::domain::errors::DomainError;

/// Version marker for the persisted model format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelVersion {
    V1,
}

/// The persisted representation of an installed API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiModel {
    pub name: String,
    pub base_url: String,
    pub version: ModelVersion,
    #[serde(default)]
    pub schema_registry: BTreeMap<String, SchemaSpec>,
    pub operation_groups: Vec<ApiOperationGroup>,
}

impl ApiModel {
    /// Returns the registry schema for the given ref id, if present.
    pub fn schema_by_ref_id(&self, ref_id: &str) -> Option<&SchemaSpec> {
        self.schema_registry.get(ref_id)
    }
}

/// A named group of operations derived from an OpenAPI tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiOperationGroup {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub operations: Vec<ApiOperation>,
}

/// A single API operation (endpoint) exposed as a CLI command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiOperation {
    pub name: String,
    pub summary: Option<String>,
    pub method: HttpMethod,
    pub path: String,
    pub path_params: Vec<Param>,
    pub query_params: Vec<Param>,
    pub request_body: Option<BodySpec>,
    #[serde(default)]
    pub responses: Vec<ResponseSpec>,
}

/// A path or query parameter, keeping both the original and CLI names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub canonical_name: String,
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// Request body metadata; the typed schema is stored for help display only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySpec {
    pub required: bool,
    pub content_type: String,
    pub schema: Option<SchemaSpec>,
}

/// An operation response body captured by status code and content type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseSpec {
    pub status_code: String,
    pub content_type: String,
    pub schema: Option<SchemaSpec>,
}

/// A property of an object schema: its own schema plus requiredness and
/// description derived from the surrounding object schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SchemaProperty {
    pub schema: SchemaSpec,
    pub required: bool,
    #[serde(default)]
    pub description: Option<String>,
}

/// A typed OpenAPI schema. Local `#/components/schemas/...` references are
/// stored as `Ref` pointing at the model's schema registry (never inlined);
/// anything unrepresentable is preserved verbatim in `Unknown`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaSpec {
    Ref {
        ref_id: String,
    },
    Object {
        properties: BTreeMap<String, SchemaProperty>,
    },
    Array {
        items: Option<Box<SchemaSpec>>,
    },
    Integer,
    Number,
    String,
    Boolean,
    Composite {
        schemas: Vec<SchemaSpec>,
    },
    Unknown {
        raw_json: String,
    },
}

/// HTTP method of an operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum HttpMethod {
    Get,
    Put,
    Post,
    Delete,
    Patch,
    Head,
    Options,
    Trace,
}

/// An outbound HTTP request produced by the request builder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: HttpMethod,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Option<Vec<u8>>,
}

/// The response to an outbound HTTP request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// A fully-resolved operation invocation: the selected operation plus input data.
#[derive(Debug, PartialEq, Eq)]
pub struct ApiInvocationRequest<'m> {
    pub base_url: String,
    pub operation: &'m ApiOperation,
    pub params: HashMap<String, Vec<String>>,
    pub body: Option<Vec<u8>>,
}

impl<'m> ApiInvocationRequest<'m> {
    pub fn new(
        base_url: String,
        operation: &'m ApiOperation,
        params: HashMap<String, Vec<String>>,
        body: Option<Vec<u8>>,
    ) -> Self {
        Self {
            base_url,
            operation,
            params,
            body,
        }
    }
}

impl HttpMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Put => "PUT",
            Self::Post => "POST",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
            Self::Trace => "TRACE",
        }
    }
}

/// Validates an install name: non-empty and safe to use as a file name.
pub fn validate_name(name: &str) -> Result<(), DomainError> {
    if name.is_empty() {
        return Err(DomainError::InvalidName {
            name: name.to_owned(),
            reason: "name must not be empty".to_owned(),
        });
    }
    if name.contains('/') || name.contains('\\') {
        return Err(DomainError::InvalidName {
            name: name.to_owned(),
            reason: "name must not contain path separators".to_owned(),
        });
    }
    if name == "." || name == ".." {
        return Err(DomainError::InvalidName {
            name: name.to_owned(),
            reason: "name must not be '.' or '..'".to_owned(),
        });
    }
    Ok(())
}
