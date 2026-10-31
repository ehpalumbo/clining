//! Pure request builder: path substitution, query serialization, body handling (Phase 3).

use std::collections::HashMap;

use url::Url;

use crate::domain::errors::DomainError;
use crate::domain::headers::{CustomHeader, is_forbidden_header_name};
use crate::domain::model::{ApiOperation, HttpRequest, ResponseSpec};

/// Builds a fully-specified `HttpRequest` from an operation, supplied values,
/// an optional body, an optional resolved bearer token, and any custom headers.
/// Values are keyed by the CLI parameter name (`Param::cli_name`); path and
/// query parameters are resolved back to their original spec names during
/// substitution. When `token` is `Some`, an `Authorization: Bearer <token>`
/// header is appended (the token must already be resolved; this function
/// performs no I/O). The `Accept` header is derived from the operation's
/// declared success-response media types (see `accept_header`). Custom headers
/// override generated headers case-insensitively and are appended in input
/// order, one pair per occurrence.
pub fn build_request(
    base_url: &str,
    operation: &ApiOperation,
    values: &HashMap<String, Vec<String>>,
    body: Option<&[u8]>,
    token: Option<&str>,
    custom_headers: &[CustomHeader],
) -> Result<HttpRequest, DomainError> {
    let body = body.filter(|b| !b.is_empty());
    let url = build_url(base_url, operation, values)?;

    let mut headers: Vec<(String, String)> = vec![
        (
            "User-Agent".to_owned(),
            format!("clining/{}", env!("CARGO_PKG_VERSION")),
        ),
        ("Accept".to_owned(), accept_header(&operation.responses)),
    ];
    if let Some(token) = token {
        headers.push(("Authorization".to_owned(), format!("Bearer {token}")));
    }

    match &operation.request_body {
        Some(spec) => {
            if body.is_some() {
                let content_type = if spec.content_type.is_empty() {
                    "application/json"
                } else {
                    &spec.content_type
                };
                headers.push(("Content-Type".to_owned(), content_type.to_owned()));
            } else if spec.required {
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

    let headers = merge_headers(headers, custom_headers)?;
    Ok(HttpRequest {
        method: operation.method,
        url,
        headers,
        body: body.map(<[u8]>::to_vec),
    })
}

/// Derives the `Accept` header value from an operation's response specs.
///
/// Success (2xx) response media types are collected in first-seen order and
/// deduplicated case-insensitively into a comma-separated list. Wildcard
/// media types (for example `application/*`) pass through as declared. When
/// the operation declares no success body — status-only responses such as
/// `204`, or stored models captured before response content was recorded —
/// `*/*` is sent so the server picks its default representation instead of
/// rejecting the negotiated types with `406 Not Acceptable`.
fn accept_header(responses: &[ResponseSpec]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    for response in responses {
        if !is_success_status(&response.status_code) {
            continue;
        }
        if let Some(content_type) = &response.content_type
            && !seen
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(content_type))
        {
            seen.push(content_type);
        }
    }
    if seen.is_empty() {
        "*/*".to_owned()
    } else {
        seen.join(", ")
    }
}

/// Whether a response status-code key denotes a success (2xx) response:
/// an exact code in `200..=299` or the `2XX` range wildcard. The `default`
/// key is ignored because it usually documents error responses.
fn is_success_status(status_code: &str) -> bool {
    if status_code.eq_ignore_ascii_case("2XX") {
        return true;
    }
    status_code.len() == 3
        && status_code.as_bytes()[0] == b'2'
        && status_code.as_bytes()[1..3].iter().all(u8::is_ascii_digit)
}

/// Merges custom headers into the generated header list.
///
/// Generated headers whose names match a custom header (ASCII
/// case-insensitive) are removed, then every custom pair is appended in input
/// order. Repeated custom names intentionally remain distinct entries so they
/// are sent as separate header lines.
fn merge_headers(
    generated: Vec<(String, String)>,
    custom: &[CustomHeader],
) -> Result<Vec<(String, String)>, DomainError> {
    for header in custom {
        if is_forbidden_header_name(&header.name) {
            return Err(DomainError::Header {
                message: format!("header '{}' is not allowed", header.name),
            });
        }
    }

    let mut merged: Vec<(String, String)> = generated
        .into_iter()
        .filter(|(name, _)| !custom.iter().any(|h| h.name.eq_ignore_ascii_case(name)))
        .collect();
    merged.extend(
        custom
            .iter()
            .map(|header| (header.name.clone(), header.value.clone())),
    );
    Ok(merged)
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

    use crate::domain::model::{BodySpec, HttpMethod, Param, ResponseSpec};

    fn build(
        base_url: &str,
        operation: &ApiOperation,
        values: &HashMap<String, Vec<String>>,
        body: Option<&[u8]>,
        token: Option<&str>,
    ) -> Result<HttpRequest, DomainError> {
        build_request(base_url, operation, values, body, token, &[])
    }

    fn response(status_code: &str, content_type: Option<&str>) -> ResponseSpec {
        ResponseSpec {
            status_code: status_code.to_owned(),
            description: None,
            content_type: content_type.map(str::to_owned),
            schema: None,
        }
    }

    fn with_responses(operation: &ApiOperation, responses: Vec<ResponseSpec>) -> ApiOperation {
        let mut operation = operation.clone();
        operation.responses = responses;
        operation
    }

    fn operation_with(
        path: &str,
        path_params: Vec<Param>,
        query_params: Vec<Param>,
        request_body: Option<BodySpec>,
    ) -> ApiOperation {
        ApiOperation {
            name: "test".to_owned(),
            summary: None,
            description: None,
            method: HttpMethod::Post,
            path: path.to_owned(),
            path_params,
            query_params,
            request_body,
            responses: vec![],
        }
    }

    fn pet_id() -> Param {
        Param {
            name: "petId".to_owned(),
            canonical_name: "pet-id".to_owned(),
            required: true,
            description: None,
            schema: None,
        }
    }

    fn status() -> Param {
        Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
            schema: None,
        }
    }

    #[test]
    fn substitutes_path_and_query() {
        let operation = operation_with("/pets/{petId}", vec![pet_id()], vec![status()], None);
        let values = HashMap::from([
            ("pet-id".to_owned(), vec!["123".to_owned()]),
            ("status".to_owned(), vec!["available".to_owned()]),
        ]);
        let req = build(
            "https://api.example.com/v1/",
            &operation,
            &values,
            None,
            None,
        )
        .unwrap();
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
        // No declared success responses (legacy stored model): the wildcard
        // fallback lets the server pick its default representation.
        assert!(req.headers.iter().any(|(k, v)| k == "Accept" && v == "*/*"));
    }

    #[test]
    fn accept_matches_declared_success_media_type() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![response("200", Some("text/csv"))],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert!(accept_of(&req).contains("text/csv"));
    }

    #[test]
    fn accept_union_collects_media_types_across_success_statuses() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![
                response("200", Some("application/json")),
                response("206", Some("text/csv")),
            ],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "application/json, text/csv");
    }

    #[test]
    fn accept_deduplicates_case_insensitively_in_first_seen_order() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![
                response("200", Some("application/json")),
                response("201", Some("APPLICATION/JSON")),
                response("202", Some("text/plain")),
            ],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "application/json, text/plain");
    }

    #[test]
    fn accept_ignores_error_statuses_and_default() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![
                response("default", Some("application/problem+json")),
                response("404", Some("application/json")),
                response("4XX", Some("text/html")),
            ],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "*/*");
    }

    #[test]
    fn accept_treats_2xx_wildcard_status_as_success() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![response("2XX", Some("text/csv"))],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "text/csv");
    }

    #[test]
    fn accept_falls_back_to_wildcard_when_no_success_body_declared() {
        let operation = with_responses(
            &operation_with("/ping", vec![], vec![], None),
            vec![response("204", None)],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "*/*");
    }

    #[test]
    fn accept_passes_wildcard_media_types_through() {
        let operation = with_responses(
            &operation_with("/pets", vec![], vec![], None),
            vec![response("200", Some("application/*"))],
        );
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(accept_of(&req), "application/*");
    }

    fn accept_of(req: &HttpRequest) -> String {
        req.headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("accept"))
            .map(|(_, v)| v.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    #[test]
    fn repeated_query_values_become_repeated_keys() {
        let operation = operation_with("/pets", vec![], vec![status()], None);
        let values = HashMap::from([(
            "status".to_owned(),
            vec!["available".to_owned(), "sold".to_owned()],
        )]);
        let req = build("https://api.example.com", &operation, &values, None, None).unwrap();
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
        let req = build("https://api.example.com", &operation, &values, None, None).unwrap();
        assert_eq!(
            req.url,
            "https://api.example.com/pets/a%2Fb%20c?status=x%26y%3D1"
        );
    }

    #[test]
    fn missing_required_path_param_is_error() {
        let operation = operation_with("/pets/{petId}", vec![pet_id()], vec![], None);
        let err = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
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
            schema: None,
        };
        let operation = operation_with("/pets", vec![], vec![required], None);
        let err = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
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
            schema: None,
        };
        let operation = operation_with("/pets/{petId}", vec![optional], vec![], None);
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert_eq!(req.url, "https://api.example.com/pets/");
    }

    #[test]
    fn required_body_missing_is_error() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            description: None,
            schema: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let err = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("request body"), "{err}");
    }

    #[test]
    fn body_attached_with_content_type() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            description: None,
            schema: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b"{\"name\":\"fluffy\"}"),
            None,
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
            description: None,
            schema: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b"{}"),
            None,
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
        let err = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b"{}"),
            None,
        )
        .unwrap_err();
        assert!(err.to_string().contains("no request body"), "{err}");
    }

    #[test]
    fn empty_body_treated_as_no_body() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b""),
            None,
        )
        .unwrap();
        assert_eq!(req.body, None);
    }

    #[test]
    fn invalid_base_url_is_parameter_error() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let err = build("not a url", &operation, &HashMap::new(), None, None).unwrap_err();
        assert!(matches!(err, DomainError::Parameter { .. }), "{err}");
        assert!(err.to_string().contains("base URL"), "{err}");
    }

    #[test]
    fn bearer_token_adds_authorization_header() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            Some("secret-123"),
        )
        .unwrap();
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer secret-123")
        );
    }

    #[test]
    fn no_token_means_no_authorization_header() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let req = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        assert!(!req.headers.iter().any(|(k, _)| k == "Authorization"));
    }

    fn header(name: &str, value: &str) -> CustomHeader {
        CustomHeader {
            name: name.to_owned(),
            value: value.to_owned(),
        }
    }

    #[test]
    fn custom_headers_override_generated_headers_case_insensitively() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let custom = vec![
            header("user-agent", "custom-agent/1"),
            header("Accept", "text/plain"),
            header("authorization", "Token abc"),
        ];
        let req = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            Some("secret-123"),
            &custom,
        )
        .unwrap();

        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "user-agent" && v == "custom-agent/1")
        );
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "Accept" && v == "text/plain")
        );
        assert!(
            req.headers
                .iter()
                .any(|(k, v)| k == "authorization" && v == "Token abc")
        );
        assert!(
            !req.headers
                .iter()
                .any(|(k, v)| k == "User-Agent" && v.starts_with("clining/"))
        );
        assert!(
            !req.headers
                .iter()
                .any(|(k, v)| k == "Accept" && v == "application/json")
        );
        assert!(
            !req.headers
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer secret-123")
        );
    }

    #[test]
    fn custom_content_type_replaces_generated_one() {
        let spec = BodySpec {
            required: true,
            content_type: "application/json".to_owned(),
            description: None,
            schema: None,
        };
        let operation = operation_with("/pets", vec![], vec![], Some(spec));
        let custom = vec![header("content-type", "text/plain")];
        let req = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            Some(b"hello"),
            None,
            &custom,
        )
        .unwrap();
        assert_eq!(
            req.headers
                .iter()
                .filter(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.as_str())
                .collect::<Vec<_>>(),
            vec!["text/plain"]
        );
        assert_eq!(req.body, Some(b"hello".to_vec()));
    }

    #[test]
    fn repeated_custom_names_remain_separate_in_input_order() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let custom = vec![header("X-Thing", "first"), header("x-thing", "second")];
        let req = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
            &custom,
        )
        .unwrap();
        let values: Vec<&str> = req
            .headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("x-thing"))
            .map(|(_, v)| v.as_str())
            .collect();
        assert_eq!(values, vec!["first", "second"]);
    }

    #[test]
    fn empty_custom_headers_leave_generated_list_unchanged() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let baseline = build(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
        )
        .unwrap();
        let with_custom = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(with_custom.headers, baseline.headers);
    }

    #[test]
    fn denylisted_custom_header_is_rejected_by_the_builder() {
        let operation = operation_with("/pets", vec![], vec![], None);
        let custom = vec![header("cOnNeCtIoN", "close")];
        let err = build_request(
            "https://api.example.com",
            &operation,
            &HashMap::new(),
            None,
            None,
            &custom,
        )
        .unwrap_err();
        assert!(matches!(err, DomainError::Header { .. }), "{err}");
        assert!(!err.to_string().contains("close"), "{err}");
    }
}
