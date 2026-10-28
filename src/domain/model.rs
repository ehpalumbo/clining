//! Persisted API model entities (Phase 2).

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::domain::errors::DomainError;
use crate::domain::headers::CustomHeader;

/// Version marker for the persisted model format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelVersion {
    V1,
}

/// The persisted representation of an installed API.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiModel {
    pub name: String,
    pub base_url: String,
    pub version: ModelVersion,
    pub schema_registry: BTreeMap<String, SchemaSpec>,
    pub auth: Option<AuthConfig>,
    pub operation_groups: Vec<ApiOperationGroup>,
}

/// Bearer token source reference persisted in the model.
///
/// Only the *reference* (env var name or file path) is stored; the secret
/// itself is resolved at invocation time and never written to disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum AuthConfig {
    Env { var: String },
    File { path: String },
}

impl ApiModel {
    /// Returns the registry schema for the given ref id, if present.
    pub fn schema_by_ref_id(&self, ref_id: &str) -> Option<&SchemaSpec> {
        self.schema_registry.get(ref_id)
    }
}

/// A named group of operations derived from an OpenAPI tag.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiOperationGroup {
    pub name: String,
    pub description: Option<String>,
    pub operations: Vec<ApiOperation>,
}

/// A single API operation (endpoint) exposed as a CLI command.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiOperation {
    pub name: String,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub method: HttpMethod,
    pub path: String,
    pub path_params: Vec<Param>,
    pub query_params: Vec<Param>,
    pub request_body: Option<BodySpec>,
    pub responses: Vec<ResponseSpec>,
}

/// A path or query parameter, keeping both the original and CLI names.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub canonical_name: String,
    pub required: bool,
    pub description: Option<String>,
    /// Value shape from the spec, captured for `--help` when present.
    pub schema: Option<SchemaSpec>,
}

/// Request body metadata; the typed schema is stored for help display only.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySpec {
    pub required: bool,
    pub content_type: String,
    pub description: Option<String>,
    pub schema: Option<SchemaSpec>,
}

/// An operation response body captured by status code and content type.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResponseSpec {
    pub status_code: String,
    pub description: Option<String>,
    /// Body content type, or `None` when the response declares no body (for
    /// example `204`).
    pub content_type: Option<String>,
    pub schema: Option<SchemaSpec>,
}

/// Composite schema kind (logical combination of schemas).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum CompositeKind {
    AllOf,
    OneOf,
    AnyOf,
}

impl CompositeKind {
    pub fn keyword(&self) -> &'static str {
        match self {
            Self::AllOf => "allOf",
            Self::OneOf => "oneOf",
            Self::AnyOf => "anyOf",
        }
    }
}

/// A typed schema representation. Local `#/components/schemas/...` references
/// are stored as `Ref` pointing at the model's schema registry (never inlined).
/// Leaf nodes preserve their complete `raw_json` payload, while compound nodes
/// store child schemas structurally and preserve only unmodeled metadata in
/// `extra_json` to avoid duplication.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SchemaSpec {
    Ref {
        ref_id: String,
    },
    Object {
        #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
        properties: BTreeMap<String, SchemaSpec>,
        extra_json: Option<String>,
    },
    Array {
        items: Option<Box<SchemaSpec>>,
        extra_json: Option<String>,
    },
    Primitive {
        raw_json: String,
    },
    Composite {
        composite_kind: CompositeKind,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        schemas: Vec<SchemaSpec>,
        extra_json: Option<String>,
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
    pub auth: Option<AuthConfig>,
    pub headers: Vec<CustomHeader>,
}

impl<'m> ApiInvocationRequest<'m> {
    pub fn new(
        base_url: String,
        operation: &'m ApiOperation,
        params: HashMap<String, Vec<String>>,
        body: Option<Vec<u8>>,
        auth: Option<AuthConfig>,
        headers: Vec<CustomHeader>,
    ) -> Self {
        Self {
            base_url,
            operation,
            params,
            body,
            auth,
            headers,
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
