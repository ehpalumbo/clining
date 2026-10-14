//! Bearer token resolution for `AuthConfig` references.
//!
//! Tokens are always resolved at invocation time (and validated at install
//! time) from a local source; the secret value is never persisted, logged,
//! or included in error messages.

use crate::domain::errors::DomainError;
use crate::domain::model::AuthConfig;
use std::io::Read;

/// Maximum token file size read (64 KiB). Bearer tokens are small; this
/// bounds memory use on misconfigured paths.
const MAX_TOKEN_FILE_BYTES: u64 = 64 * 1024;

/// Builds an `AuthConfig` from install flag values.
///
/// Returns `None` when neither flag is supplied (re-install without flags
/// clears auth). Returns a `Parameter` error when both are supplied or when
/// a supplied value is empty.
pub fn auth_from_options(
    token_env: Option<&str>,
    token_file: Option<&str>,
) -> Result<Option<AuthConfig>, DomainError> {
    match (token_env, token_file) {
        (Some(_), Some(_)) => Err(DomainError::Parameter {
            message: "only one of --token-env or --token-file may be supplied".to_owned(),
        }),
        (Some(var), None) => {
            if var.is_empty() {
                return Err(DomainError::Parameter {
                    message: "token env var name must not be empty".to_owned(),
                });
            }
            Ok(Some(AuthConfig::Env {
                var: var.to_owned(),
            }))
        }
        (None, Some(path)) => {
            if path.is_empty() {
                return Err(DomainError::Parameter {
                    message: "token file path must not be empty".to_owned(),
                });
            }
            Ok(Some(AuthConfig::File {
                path: path.to_owned(),
            }))
        }
        (None, None) => Ok(None),
    }
}

/// Resolves the bearer token value for an auth reference.
///
/// Env values are used verbatim (empty is an error). File contents are
/// trimmed of trailing `\n`/`\r` only; empty-after-trim is an error.
/// Tokens containing control characters are rejected to prevent header
/// injection. Error messages reference only the var name or file path,
/// never the secret.
pub fn resolve_token(auth: &AuthConfig) -> Result<String, DomainError> {
    match auth {
        AuthConfig::Env { var } => resolve_env(var, &|key| std::env::var(key)),
        AuthConfig::File { path } => resolve_file(path),
    }
}

fn resolve_env(
    var: &str,
    lookup: &dyn Fn(&str) -> Result<String, std::env::VarError>,
) -> Result<String, DomainError> {
    match lookup(var) {
        Err(std::env::VarError::NotPresent) => Err(DomainError::Auth {
            message: format!("environment variable '{var}' is not set"),
        }),
        Err(std::env::VarError::NotUnicode(_)) => Err(DomainError::Auth {
            message: format!("environment variable '{var}' value is not valid UTF-8"),
        }),
        Ok(value) => {
            if value.is_empty() {
                return Err(DomainError::Auth {
                    message: format!("environment variable '{var}' is empty"),
                });
            }
            reject_invalid_header_value(&value).map_err(|_| DomainError::Auth {
                message: format!(
                    "environment variable '{var}' contains an invalid newline or control character"
                ),
            })?;
            Ok(value)
        }
    }
}

fn resolve_file(path: &str) -> Result<String, DomainError> {
    let metadata = std::fs::metadata(path).map_err(|e| DomainError::Auth {
        message: format!("failed to read token file '{path}': {e}"),
    })?;
    if metadata.len() > MAX_TOKEN_FILE_BYTES {
        return Err(DomainError::Auth {
            message: format!("token file '{path}' exceeds size limit"),
        });
    }
    let raw = {
        let file = std::fs::File::open(path).map_err(|e| DomainError::Auth {
            message: format!("failed to read token file '{path}': {e}"),
        })?;
        let mut raw = String::new();
        file.take(MAX_TOKEN_FILE_BYTES + 1)
            .read_to_string(&mut raw)
            .map_err(|e| DomainError::Auth {
                message: format!("failed to read token file '{path}': {e}"),
            })?;
        if raw.len() as u64 > MAX_TOKEN_FILE_BYTES {
            return Err(DomainError::Auth {
                message: format!("token file '{path}' exceeds size limit"),
            });
        }
        raw
    };
    let trimmed = raw.trim_end_matches(['\n', '\r']).to_owned();
    if trimmed.is_empty() {
        return Err(DomainError::Auth {
            message: format!("token file '{path}' is empty"),
        });
    }
    reject_invalid_header_value(&trimmed).map_err(|_| DomainError::Auth {
        message: format!("token file '{path}' contains an invalid newline or control character"),
    })?;
    Ok(trimmed)
}

fn reject_invalid_header_value(token: &str) -> Result<(), ()> {
    if token.bytes().any(|b| b < 0x20 || b == 0x7F) {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn both_sources_is_parameter_error() {
        let err = auth_from_options(Some("A"), Some("/tmp/t")).unwrap_err();
        assert!(matches!(err, DomainError::Parameter { .. }));
    }

    #[test]
    fn neither_source_is_none() {
        assert_eq!(auth_from_options(None, None).unwrap(), None);
    }

    #[test]
    fn empty_values_are_rejected() {
        assert!(auth_from_options(Some(""), None).is_err());
        assert!(auth_from_options(None, Some("")).is_err());
    }

    #[test]
    fn env_lookup_success_missing_and_empty() {
        let token = resolve_env("ANY_VAR", &|_| Ok("secret-123".to_owned())).unwrap();
        assert_eq!(token, "secret-123");

        let err = resolve_env("MISSING_VAR", &|_| Err(std::env::VarError::NotPresent)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("MISSING_VAR"), "{msg}");
        assert!(msg.contains("not set"), "{msg}");
        assert!(!msg.contains("secret-123"), "{msg}");

        let err = resolve_env("EMPTY_VAR", &|_| Ok(String::new())).unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
    }

    #[test]
    fn env_non_utf8_is_distinct_from_not_set() {
        let err = resolve_env("NON_UTF8_VAR", &|_| {
            Err(std::env::VarError::NotUnicode(std::ffi::OsString::from(
                "bad-value",
            )))
        })
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("NON_UTF8_VAR"), "{msg}");
        assert!(!msg.contains("not set"), "{msg}");
        assert!(!msg.contains("bad-value"), "{msg}");
    }

    #[test]
    fn env_missing_var_via_resolve_token() {
        // Uses a name that is never set; no env mutation required.
        let var = "CLINING_TEST_DEFINITELY_UNSET_VAR_XYZ_123";
        let err = resolve_token(&AuthConfig::Env {
            var: var.to_owned(),
        })
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(err.to_string().contains(var));
    }

    fn write_temp(contents: &[u8]) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "clining-token-{}-{}-{}.txt",
            std::process::id(),
            n,
            contents.len()
        ));
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(contents).unwrap();
        path
    }

    #[test]
    fn file_trims_trailing_newline() {
        let path = write_temp(b"abc123\n");
        let token = resolve_token(&AuthConfig::File {
            path: path.display().to_string(),
        })
        .unwrap();
        assert_eq!(token, "abc123");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn file_trims_crlf_and_keeps_inner_whitespace() {
        let path = write_temp(b"tok \r\n");
        // Trailing \r\n trimmed; trailing space before it is preserved verbatim.
        let token = resolve_token(&AuthConfig::File {
            path: path.display().to_string(),
        })
        .unwrap();
        assert_eq!(token, "tok ");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn file_missing_and_empty_are_errors_without_secret() {
        let err = resolve_token(&AuthConfig::File {
            path: "/nonexistent-clining-token-xyz".to_owned(),
        })
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }));

        let path = write_temp(b"\n");
        let err = resolve_token(&AuthConfig::File {
            path: path.display().to_string(),
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty"), "{msg}");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn file_oversize_is_rejected_with_size_limit() {
        let big = vec![b'a'; MAX_TOKEN_FILE_BYTES as usize + 1];
        let path = write_temp(&big);
        let err = resolve_token(&AuthConfig::File {
            path: path.display().to_string(),
        })
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains("size limit"), "{msg}");
        assert!(msg.contains(&path.display().to_string()), "{msg}");
        std::fs::remove_file(&path).unwrap();
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn dev_zero_is_rejected_with_size_limit_without_hanging() {
        let err = resolve_token(&AuthConfig::File {
            path: "/dev/zero".to_owned(),
        })
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(err.to_string().contains("size limit"), "{err}");
    }

    #[test]
    fn control_characters_are_rejected_for_env_and_file() {
        for token in ["ab\x00cd", "ab\x01cd", "ab\x7Fcd", "ab\ncd", "ab\rcd"] {
            let err = resolve_env("CTL_VAR", &|_| Ok(token.to_owned())).unwrap_err();
            assert!(matches!(err, DomainError::Auth { .. }), "{token:?}: {err}");
            let msg = err.to_string();
            assert!(msg.contains("CTL_VAR"), "{token:?}: {msg}");
            assert!(!msg.contains(token), "{token:?}: {msg}");

            let path = write_temp(token.as_bytes());
            let err = resolve_token(&AuthConfig::File {
                path: path.display().to_string(),
            })
            .unwrap_err();
            assert!(matches!(err, DomainError::Auth { .. }), "{token:?}: {err}");
            assert!(!matches!(err, DomainError::Network { .. }), "{token:?}");
            std::fs::remove_file(&path).unwrap();
        }
    }

    #[allow(unsafe_code)]
    #[test]
    fn env_control_characters_via_resolve_token_are_auth_errors() {
        // NUL cannot appear in a real process environment (execve rejects it),
        // so exercise it through the mocked lookup above; here cover CTLs that
        // are settable via the environment.
        for suspicious in ["bad\x01token", "bad\x7Ftoken", "bad\ntoken"] {
            let var = "CLINING_TEST_CTL_ENV_XYZ_123";
            unsafe {
                std::env::set_var(var, suspicious);
            }
            let err = resolve_token(&AuthConfig::Env {
                var: var.to_owned(),
            })
            .unwrap_err();
            assert!(matches!(err, DomainError::Auth { .. }), "{err}");
            assert!(!matches!(err, DomainError::Network { .. }), "{err}");
            assert!(err.to_string().contains(var));
            unsafe {
                std::env::remove_var(var);
            }
        }
    }
}
