//! "Invoke operation" use case: build and send the HTTP request for an
//! `ApiInvocationRequest` (Phase 3).

use crate::application::request_builder::build_request;
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiInvocationRequest, HttpResponse};
use crate::domain::ports::HttpInvoker;

/// Sends a fully-resolved operation invocation.
pub struct InvokeOperationService<I>
where
    I: HttpInvoker,
{
    invoker: I,
}

impl<I> InvokeOperationService<I>
where
    I: HttpInvoker,
{
    pub fn new(invoker: I) -> Self {
        Self { invoker }
    }

    /// Builds the HTTP request from the invocation and sends it, returning the
    /// raw response.
    pub fn invoke(
        &self,
        invocation: &ApiInvocationRequest<'_>,
    ) -> Result<HttpResponse, DomainError> {
        let request = build_request(
            &invocation.base_url,
            invocation.operation,
            &invocation.params,
            invocation.body.as_deref(),
        )?;
        self.invoker.send(&request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    use crate::domain::model::{
        ApiModel, ApiOperation, ApiOperationGroup, HttpMethod, HttpRequest, ModelVersion, Param,
    };

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://api.example.com/v1".to_owned(),
            version: ModelVersion::V1,
            operation_groups: vec![ApiOperationGroup {
                name: "pets".to_owned(),
                operations: vec![ApiOperation {
                    name: "get-pet".to_owned(),
                    summary: None,
                    method: HttpMethod::Get,
                    path: "/pets/{petId}".to_owned(),
                    path_params: vec![Param {
                        name: "petId".to_owned(),
                        cli_name: "pet-id".to_owned(),
                        required: true,
                    }],
                    query_params: vec![Param {
                        name: "status".to_owned(),
                        cli_name: "status".to_owned(),
                        required: false,
                    }],
                    request_body: None,
                }],
            }],
        }
    }

    fn operation(model: &ApiModel) -> &ApiOperation {
        &model.operation_groups[0].operations[0]
    }

    fn request<'m>(
        model: &'m ApiModel,
        params: HashMap<String, Vec<String>>,
        body: Option<Vec<u8>>,
    ) -> ApiInvocationRequest<'m> {
        ApiInvocationRequest::new(model.base_url.clone(), operation(model), params, body)
    }

    struct FakeInvoker {
        captured: Rc<RefCell<Option<HttpRequest>>>,
    }

    impl HttpInvoker for FakeInvoker {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse, DomainError> {
            *self.captured.borrow_mut() = Some(request.clone());
            Ok(HttpResponse {
                status: 200,
                headers: vec![],
                body: b"ok".to_vec(),
            })
        }
    }

    fn service() -> (
        InvokeOperationService<FakeInvoker>,
        Rc<RefCell<Option<HttpRequest>>>,
    ) {
        let captured = Rc::new(RefCell::new(None));
        let invoker = FakeInvoker {
            captured: Rc::clone(&captured),
        };
        (InvokeOperationService::new(invoker), captured)
    }

    #[test]
    fn successful_invoke_builds_and_sends() {
        let (svc, captured) = service();
        let model = sample_model();
        let params = HashMap::from([
            ("pet-id".to_owned(), vec!["42".to_owned()]),
            ("status".to_owned(), vec!["available".to_owned()]),
        ]);
        let resp = svc.invoke(&request(&model, params, None)).unwrap();
        assert_eq!(resp.status, 200);
        let sent = captured.borrow().clone().unwrap();
        assert_eq!(
            sent.url,
            "https://api.example.com/v1/pets/42?status=available"
        );
        assert_eq!(sent.method, HttpMethod::Get);
    }

    #[test]
    fn missing_required_param_propagates() {
        let (svc, _) = service();
        let model = sample_model();
        let err = svc
            .invoke(&request(&model, HashMap::new(), None))
            .unwrap_err();
        assert!(matches!(err, DomainError::Parameter { .. }), "{err}");
    }
}
