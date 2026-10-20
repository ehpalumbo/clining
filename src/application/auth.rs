//! Bearer token resolution for `AuthConfig` references.
//!
//! Tokens are always resolved at invocation time (and validated at install
//! time) from a local source; the secret value is never persisted, logged,
//! or included in error messages.

use crate::application::ports::TokenSource;
use crate::domain::errors::DomainError;
use crate::domain::model::AuthConfig;

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
///
/// I/O failures (unset var, unreadable file, size guard) arrive already
/// mapped to `DomainError::Auth` by the `TokenSource` and propagate
/// unchanged; only value validation lives here.
pub fn resolve_token(source: &dyn TokenSource, auth: &AuthConfig) -> Result<String, DomainError> {
    match auth {
        AuthConfig::Env { var } => resolve_env(source, var),
        AuthConfig::File { path } => resolve_file(source, path),
    }
}

fn resolve_env(source: &dyn TokenSource, var: &str) -> Result<String, DomainError> {
    let value = source.read_env(var)?;
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

fn resolve_file(source: &dyn TokenSource, path: &str) -> Result<String, DomainError> {
    let bytes = source.read_file(path)?;
    let raw = String::from_utf8(bytes).map_err(|e| DomainError::Auth {
        message: format!("failed to read token file '{path}': {e}"),
    })?;
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
    use std::collections::HashMap;

    struct FakeSource {
        env_ok: HashMap<String, String>,
        env_err: HashMap<String, String>,
        file_ok: HashMap<String, Vec<u8>>,
        file_err: HashMap<String, String>,
    }

    impl FakeSource {
        fn empty() -> Self {
            Self {
                env_ok: HashMap::new(),
                env_err: HashMap::new(),
                file_ok: HashMap::new(),
                file_err: HashMap::new(),
            }
        }
    }

    impl TokenSource for FakeSource {
        fn read_env(&self, var: &str) -> Result<String, DomainError> {
            if let Some(value) = self.env_ok.get(var) {
                return Ok(value.clone());
            }
            if let Some(message) = self.env_err.get(var) {
                return Err(DomainError::Auth {
                    message: message.clone(),
                });
            }
            Err(DomainError::Auth {
                message: format!("environment variable '{var}' is not set"),
            })
        }

        fn read_file(&self, path: &str) -> Result<Vec<u8>, DomainError> {
            if let Some(bytes) = self.file_ok.get(path) {
                return Ok(bytes.clone());
            }
            if let Some(message) = self.file_err.get(path) {
                return Err(DomainError::Auth {
                    message: message.clone(),
                });
            }
            Err(DomainError::Auth {
                message: format!("failed to read token file '{path}': no such file"),
            })
        }
    }

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
        let source = FakeSource {
            env_ok: HashMap::from([
                ("ANY_VAR".to_owned(), "secret-123".to_owned()),
                ("EMPTY_VAR".to_owned(), String::new()),
            ]),
            ..FakeSource::empty()
        };
        let token = resolve_token(
            &source,
            &AuthConfig::Env {
                var: "ANY_VAR".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(token, "secret-123");

        let err = resolve_token(
            &source,
            &AuthConfig::Env {
                var: "MISSING_VAR".to_owned(),
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("MISSING_VAR"), "{msg}");
        assert!(msg.contains("not set"), "{msg}");
        assert!(!msg.contains("secret-123"), "{msg}");

        let err = resolve_token(
            &source,
            &AuthConfig::Env {
                var: "EMPTY_VAR".to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
    }

    #[test]
    fn source_errors_propagate_unchanged() {
        let source = FakeSource {
            env_err: HashMap::from([(
                "BAD_VAR".to_owned(),
                "environment variable 'BAD_VAR' value is not valid UTF-8".to_owned(),
            )]),
            file_err: HashMap::from([(
                "/over-limit.txt".to_owned(),
                "token file '/over-limit.txt' exceeds size limit".to_owned(),
            )]),
            ..FakeSource::empty()
        };
        let err = resolve_token(
            &source,
            &AuthConfig::Env {
                var: "BAD_VAR".to_owned(),
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("BAD_VAR"), "{msg}");
        assert!(!msg.contains("not set"), "{msg}");

        let err = resolve_token(
            &source,
            &AuthConfig::File {
                path: "/over-limit.txt".to_owned(),
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("size limit"), "{msg}");
        assert!(msg.contains("/over-limit.txt"), "{msg}");
    }

    #[test]
    fn env_missing_var_via_resolve_token() {
        let source = FakeSource::empty();
        let var = "CLINING_TEST_DEFINITELY_UNSET_VAR_XYZ_123";
        let err = resolve_token(
            &source,
            &AuthConfig::Env {
                var: var.to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        assert!(err.to_string().contains(var));
    }

    #[test]
    fn file_trims_trailing_newline() {
        let source = FakeSource {
            file_ok: HashMap::from([("/tok.txt".to_owned(), b"abc123\n".to_vec())]),
            ..FakeSource::empty()
        };
        let token = resolve_token(
            &source,
            &AuthConfig::File {
                path: "/tok.txt".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(token, "abc123");
    }

    #[test]
    fn file_trims_crlf_and_keeps_inner_whitespace() {
        let source = FakeSource {
            file_ok: HashMap::from([("/tok.txt".to_owned(), b"tok \r\n".to_vec())]),
            ..FakeSource::empty()
        };
        // Trailing \r\n trimmed; trailing space before it is preserved verbatim.
        let token = resolve_token(
            &source,
            &AuthConfig::File {
                path: "/tok.txt".to_owned(),
            },
        )
        .unwrap();
        assert_eq!(token, "tok ");
    }

    #[test]
    fn file_missing_and_empty_are_errors_without_secret() {
        let source = FakeSource {
            file_ok: HashMap::from([("/empty.txt".to_owned(), b"\n".to_vec())]),
            ..FakeSource::empty()
        };
        let err = resolve_token(
            &source,
            &AuthConfig::File {
                path: "/nonexistent-clining-token-xyz".to_owned(),
            },
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }));

        let err = resolve_token(
            &source,
            &AuthConfig::File {
                path: "/empty.txt".to_owned(),
            },
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("empty"), "{msg}");
    }

    #[test]
    fn file_non_utf8_is_auth_error_without_secret() {
        let path = "/non-utf8.txt".to_owned();
        let source = FakeSource {
            file_ok: HashMap::from([(path.clone(), vec![0xff, 0xfe, b'a'])]),
            ..FakeSource::empty()
        };
        let err = resolve_token(&source, &AuthConfig::File { path: path.clone() }).unwrap_err();
        assert!(matches!(err, DomainError::Auth { .. }), "{err}");
        let msg = err.to_string();
        assert!(msg.contains(&path), "{msg}");
    }

    #[test]
    fn control_characters_are_rejected_for_env_and_file() {
        for token in ["ab\x00cd", "ab\x01cd", "ab\x7Fcd", "ab\ncd", "ab\rcd"] {
            let path = format!("/ctl-{}.txt", token.len());
            let source = FakeSource {
                env_ok: HashMap::from([("CTL_VAR".to_owned(), token.to_owned())]),
                file_ok: HashMap::from([(path.clone(), token.as_bytes().to_vec())]),
                ..FakeSource::empty()
            };
            let err = resolve_token(
                &source,
                &AuthConfig::Env {
                    var: "CTL_VAR".to_owned(),
                },
            )
            .unwrap_err();
            assert!(matches!(err, DomainError::Auth { .. }), "{token:?}: {err}");
            let msg = err.to_string();
            assert!(msg.contains("CTL_VAR"), "{token:?}: {msg}");
            assert!(!msg.contains(token), "{token:?}: {msg}");

            let err = resolve_token(&source, &AuthConfig::File { path: path.clone() }).unwrap_err();
            assert!(matches!(err, DomainError::Auth { .. }), "{token:?}: {err}");
            assert!(!matches!(err, DomainError::Network { .. }), "{token:?}");
        }
    }
}
