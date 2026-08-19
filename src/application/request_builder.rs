//! Pure request builder: path substitution, query serialization, body handling (Phase 3).

use std::collections::HashMap;

use url::Url;

use crate::domain::errors::DomainError;
use crate::domain::model::{ApiOperation, HttpRequest};

/// Builds a fully-specified `HttpRequest` from an operation, supplied values,
/// and an optional body. Values are keyed by the CLI parameter name
/// (`Param::cli_name`); path and query parameters are resolved back to their
/// original spec names during substitution.
pub fn build_request(
    base_url: &str,
    operation: &ApiOperation,
    values: &HashMap<String, Vec<String>>,
    body: Option<&[u8]>,
) -> Result<HttpRequest, DomainError> {
    let body = body.filter(|b| !b.is_empty());
    let url = build_url(base_url, operation, values)?;

    let mut headers: Vec<(String, String)> = vec![
        (
            "User-Agent".to_owned(),
            format!("clining/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("Accept".to_owned(), "application/json".to_owned()),
    ];

    match &operation.request_body {
        Some(spec) => {
            if let Some(bytes) = body {
                let content_type = if spec.content_type.is_empty() {
                    "application/json"
                } else {
                    &spec.content_type
                };
                headers.push(("Content-Type".to_owned(), content_type.to_owned()));
                return Ok(HttpRequest {
                    method: operation.method,
                    url,
                    headers,
                    body: Some(bytes.to_vec()),
                });
            }
            if spec.required {
                return Err(DomainError::Body {
                    message: format!("command '{}' requires a request body", operation.name),
                });
            }
        }
        None => {
            if body.is_some() {
                return Err(DomainError::Body {
                    message: format!("command '{}' declares no request body", operation.name),
                });
            }
        }
    }

    Ok(HttpRequest {
        method: operation.method,
        url,
        headers,
        body: None,
    })
}

/// Builds a URL by substituting path parameters and serializing query parameters.
fn build_url(
    base_url: &str,
    operation: &ApiOperation,
    values: &HashMap<String, Vec<String>>,
) -> Result<String, DomainError> {
    let mut url = Url::parse(base_url).map_err(|e| DomainError::Parameter {
        message: format!("invalid base URL '{base_url}': {e}"),
    })?;
    // Substitute path parameters
    let mut segments: Vec<String> = operation.path.split('/').map(str::to_owned).collect();
    for param in &operation.path_params {
        let placeholder = format!("{{{}}}", param.name);
        let value = match values.get(&param.canonical_name).and_then(|v| v.first()) {
            Some(value) => value,
            None if param.required => {
                return Err(DomainError::Parameter {
                    message: format!(
                        "missing required path parameter '{}' (--{})",
                        param.name, param.canonical_name
                    ),
                });
            }
            None => "",
        };
        for segment in &mut segments {
            *segment = segment.replace(&placeholder, value);
        }
    }
    // Remove any empty segments that may have resulted from optional parameters
    url.path_segments_mut()
        .map_err(|_| DomainError::Parameter {
            message: format!("base URL '{base_url}' cannot hold a path"),
        })?
        .pop_if_empty()
        .extend(segments.into_iter().skip_while(|s| s.is_empty()));

    // Serialize query parameters
    let mut pairs: Vec<(String, String)> = Vec::new();
    for param in &operation.query_params {
        match values.get(&param.canonical_name) {
            Some(vals) => {
                for value in vals {
                    pairs.push((param.name.clone(), value.clone()));
                }
            }
            None if param.required => {
                return Err(DomainError::Parameter {
                    message: format!(
                        "missing required query parameter '{}' (--{})",
                        param.name, param.canonical_name
                    ),
                });
            }
            None => {}
        }
    }
    if !pairs.is_empty() {
        let mut query = url.query_pairs_mut();
        for (name, value) in &pairs {
            query.append_pair(name, value);
        }
    }

    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::domain::model::{BodySpec, HttpMethod, Param};

    fn operation_with(
        path: &str,
        path_params: Vec<Param>,
        query_params: Vec<Param>,
        request_body: Option<BodySpec>,
    ) -> ApiOperation {
        ApiOperation {
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
            canonical_name: "pet-id".to_owned(),
            required: true,
            description: None,
        }
    }

    fn status() -> Param {
        Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
        }
    }

    #[test]
    fn substitutes_path_and_query() {
        let operation = operation_with("/pets/{petId}", vec![pet_id()], vec![status()], None);
        let values = HashMap::from([
            ("pet-id".to_owned(), vec!["123".to_owned()]),
            ("status".to_owned(), vec!["available".to_owned()]),
        ]);
        let req = build_request("https://api.example.com/v1/", &operation, &values, None).unwrap();
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
        let operation = operation_with("/pets", vec![], vec![status()], None);
        let values = HashMap::from([(
            "status".to_owned(),
            vec!["available".to_owned(), "sold".to_owned()],
        )]);
        let req = build_request("https://api.example.com", &operation, &values, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.example.com/pets?status=available&status=sold"
        );
    }

    #[test]
    fn encodes_reserved_characters() {
        let operation = operation_with("/pets/{petId}", vec![pet_id()], vec![status()], None);
        let values = HashMap::from([
            ("pet-id".to_owned(), vec!["a/b c".to_owned()]),
            ("status".to_owned(), vec!["x&y=1".to_owned()]),
        ]);
        let req = build_request("https://api.example.com", &operation, &values, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.example.com/pets/a%2Fb%20c?status=x%26y%3D1"
        );
    }

    #[test]
    fn missing_required_path_param_is_error() {
        let operation = operation_with("/pets/{petId}", vec![pet_id()], vec![], None);
        let err = build_request("https://api.example.com", &operation, &HashMap::new(), None)
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("petId"), "{msg}");
        assert!(msg.contains("--pet-id"), "{msg}");
    }

    #[test]
    fn missing_required_query_param_is_error() {
        let required = Param {
            name: "limit".to_owned(),
            canonical_name: "limit".to_owned(),
            required: true,
            description: None,
        };
        let operation = operation_with("/pets", vec![], vec![required], None);
        let err = build_request("https://api.example.com", &operation, &HashMap::new(), None)
            .unwrap_err();
        assert!(err.to_string().contains("limit"), "{err}");
    }

    #[test]
    fn optional_path_param_without_value_substitutes_empty() {
        let optional = Param {
            name: "petId".to_owned(),
            canonical_name: "pet-id".to_owned(),
            required: false,
            description: None,
        };
        let operation = operation_with("/pets/{petId}", vec![optional], vec![], None);
        let req =
            build_request("https://api.example.com", &operation, &HashMap::new(), None).unwrap();
        assert_eq!(req.url, "https://api.example.com/pets/");
    }

    #[test]
    fn required_body_missing_is_error() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema_json: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let err = build_request("https://api.example.com", &operation, &HashMap::new(), None)
            .unwrap_err();
        assert!(err.to_string().contains("request body"), "{err}");
    }

    #[test]
    fn body_attached_with_content_type() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            schema_json: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let req = build_request(
            "https://api.example.com",
            &operation,
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
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let req = build_request(
            "https://api.example.com",
            &operation,
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
        let operation = operation_with("/pets", vec![], vec![], None);
        let err = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b"{}"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("no request body"), "{err}");
    }

    #[test]
    fn empty_body_treated_as_no_body() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let req = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b""),
        )
        .unwrap();
        assert_eq!(req.body, None);
    }

    #[test]
    fn invalid_base_url_is_parameter_error() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let err = build_request("not a url", &operation, &HashMap::new(), None).unwrap_err();
        assert!(matches!(err, DomainError::Parameter { .. }), "{err}");
        assert!(err.to_string().contains("base URL"), "{err}");
    }
}
