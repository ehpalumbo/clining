//! Application ports implemented by infrastructure adapters.

use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, HttpRequest, HttpResponse};

/// Persistence for installed API models.
pub trait ApiStore {
    fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError>;
    fn save(&self, model: &ApiModel) -> Result<(), DomainError>;
}

/// Fetches raw spec bytes from a file path or URI.
pub trait SpecLoader {
    fn load(&self, source: &str) -> Result<Vec<u8>, DomainError>;
}

/// Parses raw OpenAPI bytes into a domain model.
pub trait OpenApiParser {
    fn parse(&self, bytes: &[u8]) -> Result<ApiModel, DomainError>;
}

/// Sends an HTTP request, returning the raw response.
pub trait HttpInvoker {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, DomainError>;
}

/// Source of bearer token values: environment variables and files.
///
/// Application policy (`auth::resolve_token`) reads through this port so that
/// token I/O stays behind an application-owned boundary (ADR-001); the only
/// production implementation lives in `crate::infra::auth`. Failures are
/// reported as `DomainError::Auth` naming only the var or path, never the
/// secret — including the infra-side size guard for misconfigured paths.
pub trait TokenSource {
    fn read_env(&self, var: &str) -> Result<String, DomainError>;
    fn read_file(&self, path: &str) -> Result<Vec<u8>, DomainError>;
}
