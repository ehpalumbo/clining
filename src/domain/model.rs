//! Persisted API model entities (Phase 2).

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
    pub command_groups: Vec<CommandGroup>,
}

/// A named group of commands derived from an OpenAPI tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandGroup {
    pub name: String,
    pub commands: Vec<Command>,
}

/// A single endpoint exposed as a CLI command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    pub summary: Option<String>,
    pub method: HttpMethod,
    pub path: String,
    pub path_params: Vec<Param>,
    pub query_params: Vec<Param>,
    pub request_body: Option<BodySpec>,
}

/// A path or query parameter, keeping both the original and CLI names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Param {
    pub name: String,
    pub cli_name: String,
    pub required: bool,
}

/// Request body metadata; the raw schema is stored for help display only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BodySpec {
    pub required: bool,
    pub content_type: String,
    pub schema_json: Option<String>,
}

/// HTTP method of a command.
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
