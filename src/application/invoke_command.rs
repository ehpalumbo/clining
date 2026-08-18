//! "Invoke command" use case: load model, resolve command, build and send request (Phase 3).

use std::collections::HashMap;

use crate::application::request_builder::build_request;
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, CommandGroup, HttpResponse};
use crate::domain::ports::{ApiStore, HttpInvoker};

/// Orchestrates model loading, command resolution, and request dispatch.
pub struct InvokeCommandService<S, I>
where
    S: ApiStore,
    I: HttpInvoker,
{
    store: S,
    invoker: I,
}

impl<S, I> InvokeCommandService<S, I>
where
    S: ApiStore,
    I: HttpInvoker,
{
    pub fn new(store: S, invoker: I) -> Self {
        Self { store, invoker }
    }

    /// Resolves `group`/`command` within the stored model for `api_name`,
    /// builds the request from `params` (keyed by CLI name) and `body`, and
    /// sends it, returning the raw response.
    pub fn invoke(
        &self,
        api_name: &str,
        group_name: &str,
        command_name: &str,
        params: &HashMap<String, Vec<String>>,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, DomainError> {
        let model = self.store.load_by_name(api_name)?;
        let group = model
            .command_groups
            .iter()
            .find(|g| g.name == group_name)
            .ok_or_else(|| unknown_group(group_name, &model))?;
        let command = group
            .commands
            .iter()
            .find(|c| c.name == command_name)
            .ok_or_else(|| unknown_command(command_name, group))?;
        let request = build_request(&model.base_url, command, params, body)?;
        self.invoker.send(&request)
    }
}

/// Returns a `DomainError::Parameter` for an unknown command group, listing valid groups.
fn unknown_group(group_name: &str, model: &ApiModel) -> DomainError {
    let names: Vec<&str> = model
        .command_groups
        .iter()
        .map(|g| g.name.as_str())
        .collect();
    DomainError::Parameter {
        message: format!(
            "unknown command group '{group_name}'; valid groups: {}",
            names.join(", ")
        ),
    }
}

/// Returns a `DomainError::Parameter` for an unknown command, listing valid commands.
fn unknown_command(command_name: &str, group: &CommandGroup) -> DomainError {
    let names: Vec<&str> = group.commands.iter().map(|c| c.name.as_str()).collect();
    DomainError::Parameter {
        message: format!(
            "unknown command '{command_name}' in group '{}'; valid commands: {}",
            group.name,
            names.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::domain::model::{Command, HttpMethod, HttpRequest, ModelVersion, Param};

    #[derive(Clone)]
    struct FakeStore {
        model: ApiModel,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                model: ApiModel {
                    name: "pets".to_owned(),
                    base_url: "https://api.example.com/v1".to_owned(),
                    version: ModelVersion::V1,
                    command_groups: vec![CommandGroup {
                        name: "pets".to_owned(),
                        commands: vec![Command {
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
                },
            }
        }
    }

    impl ApiStore for FakeStore {
        fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError> {
            if name == self.model.name {
                Ok(self.model.clone())
            } else {
                Err(DomainError::NotFound {
                    name: name.to_owned(),
                    path: "~/.clining".to_owned(),
                })
            }
        }

        fn save(&self, _model: &ApiModel) -> Result<(), DomainError> {
            Ok(())
        }
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
        InvokeCommandService<FakeStore, FakeInvoker>,
        Rc<RefCell<Option<HttpRequest>>>,
    ) {
        let captured = Rc::new(RefCell::new(None));
        let invoker = FakeInvoker {
            captured: Rc::clone(&captured),
        };
        let store = FakeStore::new();
        (InvokeCommandService::new(store, invoker), captured)
    }

    #[test]
    fn successful_invoke_builds_and_sends() {
        let (svc, captured) = service();
        let params = HashMap::from([
            ("pet-id".to_owned(), vec!["42".to_owned()]),
            ("status".to_owned(), vec!["available".to_owned()]),
        ]);
        let resp = svc
            .invoke("pets", "pets", "get-pet", &params, None)
            .unwrap();
        assert_eq!(resp.status, 200);
        let sent = captured.borrow().clone().unwrap();
        assert_eq!(
            sent.url,
            "https://api.example.com/v1/pets/42?status=available"
        );
        assert_eq!(sent.method, HttpMethod::Get);
    }

    #[test]
    fn unknown_api_is_not_found() {
        let (svc, _) = service();
        let err = svc
            .invoke("nope", "pets", "get-pet", &HashMap::new(), None)
            .unwrap_err();
        assert!(matches!(err, DomainError::NotFound { .. }));
        let msg = err.to_string();
        assert!(msg.contains("~/.clining"), "{msg}");
    }

    #[test]
    fn unknown_group_lists_valid_groups() {
        let (svc, _) = service();
        let err = svc
            .invoke("pets", "store", "get-pet", &HashMap::new(), None)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("store"), "{msg}");
        assert!(msg.contains("pets"), "{msg}");
    }

    #[test]
    fn unknown_command_lists_valid_commands() {
        let (svc, _) = service();
        let err = svc
            .invoke("pets", "pets", "nope", &HashMap::new(), None)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("nope"), "{msg}");
        assert!(msg.contains("get-pet"), "{msg}");
    }

    #[test]
    fn missing_required_param_propagates() {
        let (svc, _) = service();
        let err = svc
            .invoke("pets", "pets", "get-pet", &HashMap::new(), None)
            .unwrap_err();
        assert!(matches!(err, DomainError::Parameter { .. }), "{err}");
    }
}
