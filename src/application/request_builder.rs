//! Pure request builder: path substitution, query serialization, body handling (Phase 3).

use std::collections::HashMap;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

use crate::domain::errors::DomainError;
use crate::domain::model::{Command, HttpRequest};

/// RFC 3986 unreserved characters: alphanumerics plus `-`, `.`, `_`, `~`.
const UNRESERVED: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

/// Builds a fully-specified `HttpRequest` from a command, supplied values,
/// and an optional body. Values are keyed by the CLI parameter name
/// (`Param::cli_name`); path and query parameters are resolved back to their
/// original spec names during substitution.
pub fn build_request(
    base_url: &str,
    command: &Command,
    values: &HashMap<String, Vec<String>>,
    body: Option<&[u8]>,
) -> Result<HttpRequest, DomainError> {
    let body = body.filter(|b| !b.is_empty());
    let url = build_url(base_url, command, values)?;

    let mut headers: Vec<(String, String)> = vec![
        (
            "User-Agent".to_owned(),
            format!("clining/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];

    match &command.request_body {
        Some(spec) => {
            if let Some(bytes) = body {
                let content_type = if spec.content_type.is_empty() {
                    "application/json"
                } else {
                    &spec.content_type
                };
                headers.push(("Content-Type".to_owned(), content_type.to_owned()));
                return Ok(HttpRequest {
                    method: command.method,
                    url,
                    headers,
                    body: Some(bytes.to_vec()),
                });
            }
            if spec.required {
                return Err(DomainError::Body {
                    message: format!("command '{}' requires a request body", command.name),
                });
            }
        }
        None => {
            if body.is_some() {
                return Err(DomainError::Body {
                    message: format!("command '{}' declares no request body", command.name),
                });
            }
        }
    }

    Ok(HttpRequest {
        method: command.method,
        url,
        headers,
        body: None,
    })
}

/// Builds a URL by substituting path parameters and serializing query parameters.
fn build_url(
    base_url: &str,
    command: &Command,
    values: &HashMap<String, Vec<String>>,
) -> Result<String, DomainError> {
    // Substitute path parameters
    let mut path = command.path.clone();
    for param in &command.path_params {
        let placeholder = format!("{{{}}}", param.name);
        match values.get(&param.cli_name).and_then(|v| v.first()) {
            Some(value) => {
                path = path.replace(&placeholder, &encode(value));
            }
            None if param.required => {
                return Err(DomainError::Parameter {
                    message: format!(
                        "missing required path parameter '{}' (--{})",
                        param.name, param.cli_name
                    ),
                });
            }
            None => {
                path = path.replace(&placeholder, "");
            }
        }
    }
    // Serialize query parameters
    let mut query: Vec<String> = Vec::new();
    for param in &command.query_params {
        match values.get(&param.cli_name) {
            Some(vals) => {
                for value in vals {
                    query.push(format!("{}={}", encode(&param.name), encode(value)));
                }
            }
            None if param.required => {
                return Err(DomainError::Parameter {
                    message: format!(
                        "missing required query parameter '{}' (--{})",
                        param.name, param.cli_name
                    ),
                });
            }
            None => {}
        }
    }
    // Construct the final URL
    let base = base_url.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    let mut url = format!("{base}/{path}");
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    Ok(url)
}

/// Percent-encodes a string for use in a URL path or query parameter.
fn encode(value: &str) -> String {
    utf8_percent_encode(value, UNRESERVED).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::model::{BodySpec, HttpMethod, Param};

    fn command_with(
        path: &str,
        path_params: Vec<Param>,
        query_params: Vec<Param>,
        request_body: Option<BodySpec>,
    ) -> Command {
        Command {
            name: "test".to_owned(),
            summary: None,
            method: HttpMethod::Post,
            path: path.to_owned(),
            path_params,
            query_params,
            request_body,
        }
    }

    fn pet_id() -> Param {
        Param {
            name: "petId".to_owned(),
            cli_name: "pet-id".to_owned(),
            required: true,
        }
    }

    fn status() -> Param {
        Param {
            name: "status".to_owned(),
            cli_name: "status".to_owned(),
            required: false,
        }
    }

    #[test]
    fn substitutes_path_and_query() {
        let command = command_with("/pets/{petId}", vec![pet_id()], vec![status()], None);
        let values = HashMap::from([
            ("pet-id".to_owned(), vec!["123".to_owned()]),
            ("status".to_owned(), vec!["available".to_owned()]),
        ]);
        let req = build_request("https://api.example.com/v1/", &command, &values, None).unwrap();
        assert_eq!(req.method, HttpMethod::Post);
        assert_eq!(
            req.url,
            "https://api.example.com/v1/pets/123?status=available"
        );
        assert_eq!(req.body, None);
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "User-Agent" && v.starts_with("clining/"))
        );
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Accept" && v == "application/json")
        );
    }

    #[test]
    fn repeated_query_values_become_repeated_keys() {
        let command = command_with("/pets", vec![], vec![status()], None);
        let values = HashMap::from([(
            "status".to_owned(),
            vec!["available".to_owned(), "sold".to_owned()],
        )]);
        let req = build_request("https://api.example.com", &command, &values, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.example.com/pets?status=available&status=sold"
        );
    }

    #[test]
    fn encodes_reserved_characters() {
        let command = command_with("/pets/{petId}", vec![pet_id()], vec![status()], None);
        let values = HashMap::from([
            ("pet-id".to_owned(), vec!["a/b c".to_owned()]),
            ("status".to_owned(), vec!["x&y=1".to_owned()]),
        ]);
        let req = build_request("https://api.example.com", &command, &values, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.example.com/pets/a%2Fb%20c?status=x%26y%3D1"
        );
    }

    #[test]
    fn missing_required_path_param_is_error() {
        let command = command_with("/pets/{petId}", vec![pet_id()], vec![], None);
        let err =
            build_request("https://api.example.com", &command, &HashMap::new(), None).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("petId"), "{msg}");
        assert!(msg.contains("--pet-id"), "{msg}");
    }

    #[test]
    fn missing_required_query_param_is_error() {
        let required = Param {
            name: "limit".to_owned(),
            cli_name: "limit".to_owned(),
            required: true,
        };
        let command = command_with("/pets", vec![], vec![required], None);
        let err =
            build_request("https://api.example.com", &command, &HashMap::new(), None).unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[test]
    fn optional_path_param_without_value_substitutes_empty() {
        let optional = Param {
            name: "petId".to_owned(),
            cli_name: "pet-id".to_owned(),
            required: false,
        };
        let command = command_with("/pets/{petId}", vec![optional], vec![], None);
        let req =
            build_request("https://api.example.com", &command, &HashMap::new(), None).unwrap();
        assert_eq!(req.url, "https://api.example.com/pets/");
    }

    #[test]
    fn required_body_missing_is_error() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema_json: None,
        };
        let command = command_with("/pets", vec![], vec![], Some(spec));
        let err =
            build_request("https://api.example.com", &command, &HashMap::new(), None).unwrap_err();
        assert!(err.to_string().contains("request body"), "{err}");
    }

    #[test]
    fn body_attached_with_content_type() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema_json: None,
        };
        let command = command_with("/pets", vec![], vec![], Some(spec));
        let req = build_request(
            "https://api.example.com",
            &command,
            &HashMap::new(),
            Some(b"{\"name\":\"fluffy\"}"),
        )
        .unwrap();
        assert_eq!(req.body, Some(b"{\"name\":\"fluffy\"}".to_vec()));
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
    }

    #[test]
    fn body_content_type_falls_back_to_default() {
        let spec = BodySpec {
            required: false,
            content_type: String::new(),
            schema_json: None,
        };
        let command = command_with("/pets", vec![], vec![], Some(spec));
        let req = build_request(
            "https://api.example.com",
            &command,
            &HashMap::new(),
            Some(b"{}"),
        )
        .unwrap();
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Content-Type" && v == "application/json")
        );
    }

    #[test]
    fn unexpected_body_is_error() {
        let command = command_with("/pets", vec![], vec![], None);
        let err = build_request(
            "https://api.example.com",
            &command,
            &HashMap::new(),
            Some(b"{}"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no request body"), "{err}");
    }

    #[test]
    fn empty_body_treated_as_no_body() {
        let command = command_with("/pets", vec![], vec![], None);
        let req = build_request(
            "https://api.example.com",
            &command,
            &HashMap::new(),
            Some(b""),
        )
        .unwrap();
        assert_eq!(req.body, None);
    }
}
