//! Domain ports implemented by infrastructure adapters (Phase 2).

use crate::domain::errors::DomainError;
use crate::domain::model::ApiModel;

/// Persistence for installed API models.
pub trait ApiStore {
    #[allow(dead_code)]
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
