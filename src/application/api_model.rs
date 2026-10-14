//! API model management use cases: learn/install from spec and load installed models.

use crate::application::auth::resolve_token;
use crate::application::ports::{ApiStore, OpenApiParser, SpecLoader, TokenSource};
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, AuthConfig, validate_name};

/// Orchestrates fetching, parsing, persisting, and loading API models.
pub struct ApiModelService {
    loader: Box<dyn SpecLoader>,
    parser: Box<dyn OpenApiParser>,
    store: Box<dyn ApiStore>,
    tokens: Box<dyn TokenSource>,
}

impl ApiModelService {
    pub fn new(
        loader: impl SpecLoader + 'static,
        parser: impl OpenApiParser + 'static,
        store: impl ApiStore + 'static,
        tokens: impl TokenSource + 'static,
    ) -> Self {
        Self {
            loader: Box::new(loader),
            parser: Box::new(parser),
            store: Box::new(store),
            tokens: Box::new(tokens),
        }
    }

    /// Installs an API: validates the name, loads the spec, parses it, applies
    /// an optional base-URL override and optional auth reference, validates
    /// that the auth source is resolvable, and persists the model.
    /// `auth = None` clears any previous auth (re-install overwrite semantics).
    pub fn learn(
        &self,
        name: &str,
        source: &str,
        base_url_override: Option<&str>,
        auth: Option<AuthConfig>,
    ) -> Result<ApiModel, DomainError> {
        validate_name(name)?;
        if let Some(ref auth) = auth {
            // Fail fast at install: nothing is persisted when the source
            // cannot be resolved. The resolved value is discarded.
            let _ = resolve_token(self.tokens.as_ref(), auth)?;
        }
        let bytes = self.loader.load(source)?;
        let mut model = self.parser.parse(&bytes)?;
        model.name = name.to_owned();
        if let Some(url) = base_url_override {
            model.base_url = url.to_owned();
        }
        model.auth = auth;
        self.store.save(&model)?;
        Ok(model)
    }

    /// Loads an installed API model by name.
    pub fn load(&self, name: &str) -> Result<ApiModel, DomainError> {
        self.store.load_by_name(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    use crate::domain::model::ModelVersion;

    struct FakeLoader {
        bytes: Vec<u8>,
    }

    impl SpecLoader for FakeLoader {
        fn load(&self, _source: &str) -> Result<Vec<u8>, DomainError> {
            Ok(self.bytes.clone())
        }
    }

    struct FakeParser;

    impl OpenApiParser for FakeParser {
        fn parse(&self, bytes: &[u8]) -> Result<ApiModel, DomainError> {
            if bytes.is_empty() {
                return Err(DomainError::InvalidSpec {
                    message: "empty spec".to_owned(),
                });
            }
            Ok(ApiModel {
                name: String::new(),
                base_url: "https://from-parser.example.com".to_owned(),
                version: ModelVersion::V1,
                schema_registry: std::collections::BTreeMap::new(),
                auth: None,
                operation_groups: vec![],
            })
        }
    }

    struct FakeStore {
        saved: Rc<RefCell<Vec<ApiModel>>>,
    }

    impl FakeStore {
        fn new(saved: Rc<RefCell<Vec<ApiModel>>>) -> Self {
            Self { saved }
        }
    }

    impl ApiStore for FakeStore {
        fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError> {
            self.saved
                .borrow()
                .iter()
                .find(|m| m.name == name)
                .cloned()
                .ok_or_else(|| DomainError::NotFound {
                    name: name.to_owned(),
                    path: "fake-root".to_owned(),
                })
        }

        fn save(&self, model: &ApiModel) -> Result<(), DomainError> {
            self.saved.borrow_mut().push(model.clone());
            Ok(())
        }
    }

    fn loader(bytes: Vec<u8>) -> FakeLoader {
        FakeLoader { bytes }
    }

    struct FakeTokens {
        files: std::collections::HashMap<String, Vec<u8>>,
    }

    impl FakeTokens {
        fn empty() -> Self {
            Self {
                files: std::collections::HashMap::new(),
            }
        }
    }

    impl TokenSource for FakeTokens {
        fn read_env(&self, var: &str) -> Result<String, DomainError> {
            Err(DomainError::Auth {
                message: format!("environment variable '{var}' is not set"),
            })
        }

        fn read_file(&self, path: &str) -> Result<Vec<u8>, DomainError> {
            self.files
                .get(path)
                .cloned()
                .ok_or_else(|| DomainError::Auth {
                    message: format!("failed to read token file '{path}': no such file"),
                })
        }
    }

    #[test]
    fn install_persists_model_with_override() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(
            loader(b"{}".to_vec()),
            FakeParser,
            store,
            FakeTokens::empty(),
        );
        let model = svc
            .learn("pets", "spec.json", Some("http://override"), None)
            .unwrap();
        assert_eq!(model.name, "pets");
        assert_eq!(model.base_url, "http://override");
        assert_eq!(saved.borrow().len(), 1);
        assert_eq!(saved.borrow()[0].name, "pets");
        assert_eq!(saved.borrow()[0].base_url, "http://override");
        assert_eq!(saved.borrow()[0].auth, None);
    }

    #[test]
    fn invalid_spec_propagates_and_nothing_is_saved() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(vec![]), FakeParser, store, FakeTokens::empty());
        let err = svc.learn("pets", "spec.json", None, None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidSpec { .. }));
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn invalid_name_is_rejected() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(
            loader(b"{}".to_vec()),
            FakeParser,
            store,
            FakeTokens::empty(),
        );
        let err = svc.learn("a/b", "spec.json", None, None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidName { .. }));
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn install_with_file_auth_persists_reference_only() {
        let token_path = "/tmp/fake-learn-token.txt".to_owned();
        let tokens = FakeTokens {
            files: std::collections::HashMap::from([(
                token_path.clone(),
                b"learn-secret\n".to_vec(),
            )]),
        };
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(b"{}".to_vec()), FakeParser, store, tokens);
        let auth = Some(AuthConfig::File {
            path: token_path.clone(),
        });
        let model = svc.learn("pets", "spec.json", None, auth.clone()).unwrap();
        assert_eq!(model.auth, auth);
        // Only the reference is persisted, never the value.
        let saved_auth = saved.borrow()[0].auth.clone().unwrap();
        match saved_auth {
            AuthConfig::File { path } => assert_eq!(path, token_path),
            _ => panic!("expected file auth"),
        }
    }

    #[test]
    fn install_with_unresolvable_auth_saves_nothing() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(
            loader(b"{}".to_vec()),
            FakeParser,
            store,
            FakeTokens::empty(),
        );
        let auth = Some(AuthConfig::Env {
            var: "CLINING_TEST_MISSING_LEARN_VAR".to_owned(),
        });
        let err = svc.learn("pets", "spec.json", None, auth).unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn load_model_from_store() {
        let saved = Rc::new(RefCell::new(vec![ApiModel {
            name: "existing".to_owned(),
            base_url: "https://existing.example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: std::collections::BTreeMap::new(),
            auth: None,
            operation_groups: vec![],
        }]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(
            loader(b"{}".to_vec()),
            FakeParser,
            store,
            FakeTokens::empty(),
        );
        let model = svc.load("existing").unwrap();
        assert_eq!(model.name, "existing");

        let err = svc.load("nonexistent").unwrap_err();
        assert!(matches!(err, DomainError::NotFound { .. }));
    }
}
