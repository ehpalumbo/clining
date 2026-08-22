//! API model management use cases: learn/install from spec and load installed models.

use crate::application::ports::{ApiStore, OpenApiParser, SpecLoader};
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, validate_name};

/// Orchestrates fetching, parsing, persisting, and loading API models.
pub struct ApiModelService {
    loader: Box<dyn SpecLoader>,
    parser: Box<dyn OpenApiParser>,
    store: Box<dyn ApiStore>,
}

impl ApiModelService {
    pub fn new(
        loader: impl SpecLoader + 'static,
        parser: impl OpenApiParser + 'static,
        store: impl ApiStore + 'static,
    ) -> Self {
        Self {
            loader: Box::new(loader),
            parser: Box::new(parser),
            store: Box::new(store),
        }
    }

    /// Installs an API: validates the name, loads the spec, parses it, applies
    /// an optional base-URL override, and persists the model.
    pub fn learn(
        &self,
        name: &str,
        source: &str,
        base_url_override: Option<&str>,
    ) -> Result<ApiModel, DomainError> {
        validate_name(name)?;
        let bytes = self.loader.load(source)?;
        let mut model = self.parser.parse(&bytes)?;
        model.name = name.to_owned();
        if let Some(url) = base_url_override {
            model.base_url = url.to_owned();
        }
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

    #[test]
    fn install_persists_model_with_override() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(b"{}".to_vec()), FakeParser, store);
        let model = svc
            .learn("pets", "spec.json", Some("http://override"))
            .unwrap();
        assert_eq!(model.name, "pets");
        assert_eq!(model.base_url, "http://override");
        assert_eq!(saved.borrow().len(), 1);
        assert_eq!(saved.borrow()[0].name, "pets");
        assert_eq!(saved.borrow()[0].base_url, "http://override");
    }

    #[test]
    fn invalid_spec_propagates_and_nothing_is_saved() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(vec![]), FakeParser, store);
        let err = svc.learn("pets", "spec.json", None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidSpec { .. }));
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn invalid_name_is_rejected() {
        let saved = Rc::new(RefCell::new(vec![]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(b"{}".to_vec()), FakeParser, store);
        let err = svc.learn("a/b", "spec.json", None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidName { .. }));
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn load_model_from_store() {
        let saved = Rc::new(RefCell::new(vec![ApiModel {
            name: "existing".to_owned(),
            base_url: "https://existing.example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: std::collections::BTreeMap::new(),
            operation_groups: vec![],
        }]));
        let store = FakeStore::new(Rc::clone(&saved));
        let svc = ApiModelService::new(loader(b"{}".to_vec()), FakeParser, store);
        let model = svc.load("existing").unwrap();
        assert_eq!(model.name, "existing");

        let err = svc.load("nonexistent").unwrap_err();
        assert!(matches!(err, DomainError::NotFound { .. }));
    }
}
