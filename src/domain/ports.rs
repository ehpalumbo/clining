//! Domain ports implemented by infrastructure adapters (Phases 2-3).

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

impl<T: ApiStore + ?Sized> ApiStore for &T {
    fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError> {
        (**self).load_by_name(name)
    }

    fn save(&self, model: &ApiModel) -> Result<(), DomainError> {
        (**self).save(model)
    }
}

impl<T: HttpInvoker + ?Sized> HttpInvoker for &T {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, DomainError> {
        (**self).send(request)
    }
}
