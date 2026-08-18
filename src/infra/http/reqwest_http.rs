//! Blocking reqwest client implementing `HttpInvoker` (Phase 3).

use crate::domain::errors::DomainError;
use crate::domain::model::{HttpRequest, HttpResponse};
use crate::domain::ports::HttpInvoker;

/// Adapter that sends `HttpRequest`s over real HTTP via blocking reqwest.
pub struct ReqwestHttpClient {
    client: reqwest::blocking::Client,
}

impl ReqwestHttpClient {
    pub fn new() -> Result<Self, DomainError> {
        let client =
            reqwest::blocking::Client::builder()
                .build()
                .map_err(|e| DomainError::Network {
                    message: format!("failed to build HTTP client: {e}"),
                })?;
        Ok(Self { client })
    }
}

impl HttpInvoker for ReqwestHttpClient {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, DomainError> {
        let method =
            reqwest::Method::from_bytes(request.method.as_str().as_bytes()).map_err(|e| {
                DomainError::Network {
                    message: format!("invalid HTTP method '{}': {e}", request.method.as_str()),
                }
            })?;
        let mut builder = self.client.request(method, &request.url);
        for (name, value) in &request.headers {
            builder = builder.header(name.as_str(), value.as_str());
        }
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        let resp = builder.send().map_err(|e| DomainError::Network {
            message: format!("request to '{}' failed: {e}", request.url),
        })?;
        let status = resp.status().as_u16();
        let headers: Vec<(String, String)> =
            resp.headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_owned(),
                        value.to_str().map(str::to_owned).unwrap_or_else(|_| {
                            String::from_utf8_lossy(value.as_bytes()).into_owned()
                        }),
                    )
                })
                .collect();
        let body = resp
            .bytes()
            .map_err(|e| DomainError::Network {
                message: format!("failed to read response body from '{}': {e}", request.url),
            })?
            .to_vec();
        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};

    use crate::domain::model::HttpMethod;

    fn read_full_request(stream: &mut TcpStream) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = stream.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = headers_end(&buf) {
                let head = String::from_utf8_lossy(&buf[..end]);
                let content_length: usize = head
                    .lines()
                    .map(str::to_ascii_lowercase)
                    .filter_map(|line| {
                        line.strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .next()
                    .unwrap_or(0);
                if buf.len() >= end + 4 + content_length {
                    break;
                }
            }
        }
        buf
    }

    fn headers_end(buf: &[u8]) -> Option<usize> {
        buf.windows(4).position(|w| w == b"\r\n\r\n")
    }

    #[test]
    fn sends_request_and_roundtrips_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_full_request(&mut stream);
            let text = String::from_utf8_lossy(&request);
            assert!(
                text.starts_with("POST /pets/123?status=available HTTP/1.1"),
                "{text}"
            );
            assert!(text.contains("content-type: application/json"), "{text}");
            assert!(text.contains("user-agent: clining/"), "{text}");
            assert!(request.ends_with(b"{\"name\":\"fluffy\"}"));
            let body = b"{\"id\":\"p123\"}";
            let head = format!(
                "HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nX-Custom: yes\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });

        let client = ReqwestHttpClient::new().unwrap();
        let request = HttpRequest {
            method: HttpMethod::Post,
            url: format!("http://{addr}/pets/123?status=available"),
            headers: vec![
                ("User-Agent".to_owned(), "clining/test".to_owned()),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ],
            body: Some(b"{\"name\":\"fluffy\"}".to_vec()),
        };
        let resp = client.send(&request).unwrap();
        assert_eq!(resp.status, 201);
        assert_eq!(resp.body, b"{\"id\":\"p123\"}");
        assert!(
            resp.headers
                .iter()
                .any(|(k, v)| k == "content-type" && v == "application/json")
        );
        assert!(
            resp.headers
                .iter()
                .any(|(k, v)| k == "x-custom" && v == "yes")
        );
        handle.join().unwrap();
    }

    #[test]
    fn network_error_is_surfaced() {
        let client = ReqwestHttpClient::new().unwrap();
        let request = HttpRequest {
            method: HttpMethod::Get,
            url: "http://127.0.0.1:1/nowhere".to_owned(),
            headers: vec![],
            body: None,
        };
        let err = client.send(&request).unwrap_err();
        assert!(matches!(err, DomainError::Network { .. }), "{err}");
    }
}
