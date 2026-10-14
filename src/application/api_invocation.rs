//! "Invoke operation" use case: build and send the HTTP request for an
//! `ApiInvocationRequest`.

use crate::application::auth::resolve_token;
use crate::application::ports::HttpInvoker;
use crate::application::request_builder::build_request;
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiInvocationRequest, HttpResponse};

/// Sends a fully-resolved operation invocation.
pub struct ApiInvocationService {
    invoker: Box<dyn HttpInvoker>,
}

impl ApiInvocationService {
    pub fn new(invoker: impl HttpInvoker + 'static) -> Self {
        Self {
            invoker: Box::new(invoker),
        }
    }

    /// Builds the HTTP request from the invocation and sends it, returning the
    /// raw response. When the invocation carries an auth reference, the token
    /// is resolved first; resolution failure returns without sending.
    pub fn invoke(
        &self,
        invocation: &ApiInvocationRequest<'_>,
    ) -> Result<HttpResponse, DomainError> {
        let token = invocation.auth.as_ref().map(resolve_token).transpose()?;
        let request = build_request(
            &invocation.base_url,
            invocation.operation,
            &invocation.params,
            invocation.body.as_deref(),
            token.as_deref(),
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
        ApiModel, ApiOperation, ApiOperationGroup, AuthConfig, HttpMethod, HttpRequest,
        ModelVersion, Param,
    };

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://api.example.com/v1".to_owned(),
            version: ModelVersion::V1,
            schema_registry: std::collections::BTreeMap::new(),
            auth: None,
            operation_groups: vec![ApiOperationGroup {
                name: "pets".to_owned(),
                description: None,
                operations: vec![ApiOperation {
                    name: "get-pet".to_owned(),
                    summary: None,
                    method: HttpMethod::Get,
                    path: "/pets/{petId}".to_owned(),
                    path_params: vec![Param {
                        name: "petId".to_owned(),
                        canonical_name: "pet-id".to_owned(),
                        required: true,
                        description: None,
                    }],
                    query_params: vec![Param {
                        name: "status".to_owned(),
                        canonical_name: "status".to_owned(),
                        required: false,
                        description: None,
                    }],
                    request_body: None,
                    responses: vec![],
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
        ApiInvocationRequest::new(
            model.base_url.clone(),
            operation(model),
            params,
            body,
            model.auth.clone(),
        )
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

    fn service() -> (ApiInvocationService, Rc<RefCell<Option<HttpRequest>>>) {
        let captured = Rc::new(RefCell::new(None));
        let invoker = FakeInvoker {
            captured: Rc::clone(&captured),
        };
        (ApiInvocationService::new(invoker), captured)
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

    #[test]
    fn auth_header_attached_from_file() {
        let dir = std::env::temp_dir().join(format!("clining-invoke-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let token_path = dir.join("token.txt");
        std::fs::write(&token_path, b"file-secret\n").unwrap();
        let (svc, captured) = service();
        let mut model = sample_model();
        model.auth = Some(AuthConfig::File {
            path: token_path.display().to_string(),
        });
        let params = HashMap::from([("pet-id".to_owned(), vec!["42".to_owned()])]);
        let resp = svc.invoke(&request(&model, params, None)).unwrap();
        assert_eq!(resp.status, 200);
        let sent = captured.borrow().clone().unwrap();
        assert!(
            sent.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer file-secret")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unresolvable_auth_sends_nothing() {
        struct FailingInvoker {
            called: Rc<RefCell<bool>>,
        }
        impl HttpInvoker for FailingInvoker {
            fn send(&self, _request: &HttpRequest) -> Result<HttpResponse, DomainError> {
                *self.called.borrow_mut() = true;
                Ok(HttpResponse {
                    status: 200,
                    headers: vec![],
                    body: vec![],
                })
            }
        }
        let called = Rc::new(RefCell::new(false));
        let svc = ApiInvocationService::new(FailingInvoker {
            called: Rc::clone(&called),
        });
        let mut model = sample_model();
        model.auth = Some(AuthConfig::Env {
            var: "CLINING_TEST_MISSING_VAR_XYZ".to_owned(),
        });
        let params = HashMap::from([("pet-id".to_owned(), vec!["42".to_owned()])]);
        let err = svc.invoke(&request(&model, params, None)).unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(!*called.borrow());
    }
}
