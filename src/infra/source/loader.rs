//! Spec source loader adapter implementing the `SpecLoader` port.

use std::time::Duration;

use crate::domain::errors::DomainError;
use crate::domain::ports::SpecLoader;

/// Loads spec bytes from a local file path or an `http(s)://` URI.
pub struct SourceLoader;

impl SpecLoader for SourceLoader {
    fn load(&self, source: &str) -> Result<Vec<u8>, DomainError> {
        if source.starts_with("http://") || source.starts_with("https://") {
            self.fetch(source)
        } else {
            self.read_local_file(source)
        }
    }
}

impl SourceLoader {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, DomainError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| DomainError::Network {
                message: format!("failed to build HTTP client: {e}"),
            })?;
        let resp = client.get(url).send().map_err(|e| DomainError::Network {
            message: format!("failed to fetch {url}: {e}"),
        })?;
        if !resp.status().is_success() {
            return Err(DomainError::Network {
                message: format!("failed to fetch {url}: HTTP {}", resp.status()),
            });
        }
        let bytes = resp
            .bytes()
            .map_err(|e| DomainError::Network {
                message: format!("failed to read response from {url}: {e}"),
            })?
            .to_vec();
        Ok(bytes)
    }

    fn read_local_file(&self, path: &str) -> Result<Vec<u8>, DomainError> {
        std::fs::read(path).map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => DomainError::Io {
                message: format!("spec file not found: {path}"),
            },
            _ => DomainError::Io {
                message: format!("failed to read spec source '{path}': {e}"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn loads_local_file() {
        let dir = std::env::temp_dir().join(format!("clining-loader-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("spec.json");
        fs::write(&path, b"{\"openapi\":\"3.0.0\"}").unwrap();
        let loader = SourceLoader;
        let bytes = loader.load(path.to_str().unwrap()).unwrap();
        assert_eq!(bytes, b"{\"openapi\":\"3.0.0\"}");
    }

    #[test]
    fn missing_file_is_descriptive() {
        let loader = SourceLoader;
        let err = loader.load("/definitely/not/here/spec.json").unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("not found") || msg.contains("No such file"),
            "{msg}"
        );
    }

    #[test]
    fn loads_http_uri() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let body: &[u8] = b"{\"openapi\":\"3.0.3\"}";
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body).unwrap();
        });
        let loader = SourceLoader;
        let bytes = loader.load(&format!("http://{addr}/spec.json")).unwrap();
        assert_eq!(bytes, body);
        handle.join().unwrap();
    }
}
