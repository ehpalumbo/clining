//! JSON file store adapter implementing the `ApiStore` port.

use std::fs;
use std::path::PathBuf;

use crate::domain::errors::DomainError;
use crate::domain::model::{ApiModel, validate_name};
use crate::domain::ports::ApiStore;

/// Stores one plain JSON file per API at `<root>/<name>.json`.
pub struct JsonFileStore {
    root: PathBuf,
}

impl JsonFileStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.root.join(format!("{name}.json"))
    }
}

impl ApiStore for JsonFileStore {
    fn load_by_name(&self, name: &str) -> Result<ApiModel, DomainError> {
        let path = self.path_for(name);
        if !path.exists() {
            return Err(DomainError::NotFound {
                name: name.to_owned(),
                path: self.root.display().to_string(),
            });
        }
        let bytes = fs::read(&path).map_err(|e| DomainError::Io {
            message: format!("failed to read '{}': {e}", path.display()),
        })?;
        serde_json::from_slice(&bytes).map_err(|e| DomainError::InvalidStoredModel {
            name: name.to_owned(),
            path: path.display().to_string(),
            reason: e.to_string(),
        })
    }

    fn save(&self, model: &ApiModel) -> Result<(), DomainError> {
        validate_name(&model.name)?;
        fs::create_dir_all(&self.root).map_err(|e| DomainError::Io {
            message: format!(
                "failed to create store directory '{}': {e}",
                self.root.display()
            ),
        })?;
        let path = self.path_for(&model.name);
        let bytes = serde_json::to_vec_pretty(model).map_err(|e| DomainError::Io {
            message: format!("failed to serialize model '{}': {e}", model.name),
        })?;
        let tmp = path.with_file_name(format!(".{}.{}.tmp", model.name, std::process::id()));
        if let Err(e) = fs::write(&tmp, &bytes) {
            let _ = fs::remove_file(&tmp);
            return Err(DomainError::Io {
                message: format!("failed to write '{}': {e}", tmp.display()),
            });
        }
        fs::rename(&tmp, &path).map_err(|e| DomainError::Io {
            message: format!("failed to move '{}' into place: {e}", path.display()),
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::{ApiOperationGroup, ModelVersion};

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("clining-store-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_model(name: &str) -> ApiModel {
        ApiModel {
            name: name.to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            operation_groups: vec![ApiOperationGroup {
                name: "default".to_owned(),
                description: None,
                operations: vec![],
            }],
        }
    }

    #[test]
    fn save_then_load_roundtrips() {
        let root = temp_root("roundtrip");
        let store = JsonFileStore::new(root);
        let model = sample_model("pets");
        store.save(&model).unwrap();
        let loaded = store.load_by_name("pets").unwrap();
        assert_eq!(loaded, model);
    }

    #[test]
    fn missing_file_is_not_found() {
        let root = temp_root("missing");
        let store = JsonFileStore::new(root);
        let err = store.load_by_name("nope").unwrap_err();
        assert!(matches!(err, DomainError::NotFound { .. }));
    }

    #[test]
    fn corrupt_file_is_invalid_stored_model() {
        let root = temp_root("corrupt");
        let store = JsonFileStore::new(root.clone());
        fs::write(root.join("pets.json"), b"not json").unwrap();
        let err = store.load_by_name("pets").unwrap_err();
        assert!(matches!(err, DomainError::InvalidStoredModel { .. }));
        let msg = err.to_string();
        assert!(msg.contains("pets.json"), "{msg}");
    }

    #[test]
    fn save_replaces_corrupt_file() {
        let root = temp_root("replace");
        let store = JsonFileStore::new(root.clone());
        fs::write(root.join("pets.json"), b"corrupt").unwrap();
        store.save(&sample_model("pets")).unwrap();
        assert_eq!(store.load_by_name("pets").unwrap().name, "pets");
    }

    #[test]
    fn leaves_no_temp_files_behind() {
        let root = temp_root("atomic");
        let store = JsonFileStore::new(root.clone());
        store.save(&sample_model("pets")).unwrap();
        let entries: Vec<String> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries, vec!["pets.json"]);
    }
}
