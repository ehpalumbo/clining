//! Production `TokenSource`: environment variables and token files.

use crate::application::ports::TokenSource;
use crate::domain::errors::DomainError;
use std::io::Read;

/// Maximum token file size read (64 KiB). Bearer tokens are small; this
/// bounds memory use on misconfigured paths such as `/dev/zero` or
/// `/dev/urandom`. Kept here as an infra detail: the application policy
/// never checks lengths, it only validates already-read values.
const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;

/// Reads env vars via `std::env::var` and files via a bounded read.
///
/// `read_file` joins nothing: callers pass the stored (already absolute at
/// install time) path verbatim. Reads are bounded with
/// `take(MAX_TOKEN_FILE_BYTES + 1)` so misconfigured paths fail fast
/// instead of allocating without bound.
pub struct LocalTokenSource;

impl TokenSource for LocalTokenSource {
    fn read_env(&self, var: &str) -> Result<String, DomainError> {
        std::env::var(var).map_err(|e| map_var_error(var, e))
    }

    fn read_file(&self, path: &str) -> Result<Vec<u8>, DomainError> {
        let metadata = std::fs::metadata(path).map_err(|e| DomainError::Auth {
            message: format!("failed to read token file '{path}': {e}"),
        })?;
        if metadata.len() > MAX_TOKEN_FILE_BYTES {
            return Err(DomainError::Auth {
                message: format!("token file '{path}' exceeds size limit"),
            });
        }
        let file = std::fs::File::open(path).map_err(|e| DomainError::Auth {
            message: format!("failed to read token file '{path}': {e}"),
        })?;
        let mut limited = file.take(MAX_TOKEN_FILE_BYTES + 1);
        let mut buf = Vec::new();
        limited
            .read_to_end(&mut buf)
            .map_err(|e| DomainError::Auth {
                message: format!("failed to read token file '{path}': {e}"),
            })?;
        if buf.len() as u64 > MAX_TOKEN_FILE_BYTES {
            return Err(DomainError::Auth {
                message: format!("token file '{path}' exceeds size limit"),
            });
        }
        Ok(buf)
    }
}

fn map_var_error(var: &str, err: std::env::VarError) -> DomainError {
    match err {
        std::env::VarError::NotPresent => DomainError::Auth {
            message: format!("environment variable '{var}' is not set"),
        },
        std::env::VarError::NotUnicode(_) => DomainError::Auth {
            message: format!("environment variable '{var}' value is not valid UTF-8"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_temp(contents: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "clining-adapter-token-{}-{n}.txt",
            std::process::id()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[test]
    fn small_file_reads_verbatim() {
        let path = write_temp(b"abc123\n");
        let source = LocalTokenSource;
        let bytes = source.read_file(path.to_str().unwrap()).unwrap();
        assert_eq!(bytes, b"abc123\n");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn missing_file_is_auth_error_naming_path() {
        let source = LocalTokenSource;
        let err = source
            .read_file("/nonexistent-clining-token-xyz")
            .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("/nonexistent-clining-token-xyz"), "{msg}");
    }

    #[test]
    fn oversize_regular_file_is_size_limit_auth_error() {
        let big = vec![b'a'; MAX_TOKEN_FILE_BYTES as usize + 1];
        let path = write_temp(&big);
        let source = LocalTokenSource;
        let err = source.read_file(path.to_str().unwrap()).unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("size limit"), "{msg}");
        assert!(msg.contains(&path.display().to_string()), "{msg}");
        std::fs::remove_file(&path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dev_zero_is_bounded_size_limit_auth_error() {
        let source = LocalTokenSource;
        let err = source.read_file("/dev/zero").unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(err.to_string().contains("size limit"), "{err}");
    }

    #[test]
    fn var_error_mapping_distinguishes_not_set_from_not_unicode() {
        let err = map_var_error("MISSING_VAR", std::env::VarError::NotPresent);
        let msg = err.to_string();
        assert!(msg.contains("MISSING_VAR"), "{msg}");
        assert!(msg.contains("not set"), "{msg}");

        let err = map_var_error(
            "NON_UTF8_VAR",
            std::env::VarError::NotUnicode(std::ffi::OsString::from("bad-value")),
        );
        let msg = err.to_string();
        assert!(msg.contains("NON_UTF8_VAR"), "{msg}");
        assert!(!msg.contains("not set"), "{msg}");
        assert!(!msg.contains("bad-value"), "{msg}");
    }

    #[test]
    fn env_delegates_to_process_environment() {
        let source = LocalTokenSource;
        // Out-of-band var: compare directly against `std::env::var`, no mutation.
        assert_eq!(
            source.read_env("PATH").map_err(|e| e.to_string()),
            std::env::var("PATH").map_err(|e| map_var_error("PATH", e).to_string())
        );
        let err = source
            .read_env("CLINING_TEST_DEFINITELY_UNSET_ADAPTER_XYZ")
            .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(
            msg.contains("CLINING_TEST_DEFINITELY_UNSET_ADAPTER_XYZ"),
            "{msg}"
        );
        assert!(msg.contains("not set"), "{msg}");
    }
}
