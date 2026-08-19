//! "Learn API" use case: SpecLoader -> OpenApiParser -> ApiStore.save (Phase 2).

use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, validate_name};
use crate::domain::ports::{ApiStore, OpenApiParser, SpecLoader};

/// Orchestrates fetching, parsing, and persisting an API model.
pub struct LearnApiService<'a, L, P, S>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
{
    loader: L,
    parser: P,
    store: &'a S,
}

impl<'a, L, P, S> LearnApiService<'a, L, P, S>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
{
    pub fn new(loader: L, parser: P, store: &'a S) -> Self {
        Self {
            loader,
            parser,
            store,
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
                operation_groups: vec![],
            })
        }
    }

    struct FakeStore {
        saved: Rc<RefCell<Vec<ApiModel>>>,
    }

    impl FakeStore {
        fn new() -> Self {
            Self {
                saved: Rc::new(RefCell::new(vec![])),
            }
        }
    }

    impl ApiStore for FakeStore {
        fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError> {
            Err(DomainError::NotFound {
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
        let store = FakeStore::new();
        let saved = Rc::clone(&store.saved);
        let svc = LearnApiService::new(loader(b"{}".to_vec()), FakeParser, &store);
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
        let store = FakeStore::new();
        let saved = Rc::clone(&store.saved);
        let svc = LearnApiService::new(loader(vec![]), FakeParser, &store);
        let err = svc.learn("pets", "spec.json", None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidSpec { .. }));
        assert!(saved.borrow().is_empty());
    }

    #[test]
    fn invalid_name_is_rejected() {
        let store = FakeStore::new();
        let saved = Rc::clone(&store.saved);
        let svc = LearnApiService::new(loader(b"{}".to_vec()), FakeParser, &store);
        let err = svc.learn("a/b", "spec.json", None).unwrap_err();
        assert!(matches!(err, DomainError::InvalidName { .. }));
        assert!(saved.borrow().is_empty());
    }
}
