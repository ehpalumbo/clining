//! Custom request headers: parsing and validation rules.
//!
//! A custom header is supplied on the command line as one `Name: value` pair
//! per occurrence. The domain owns every rule here so the same validation is
//! applied at the CLI boundary and again when the request is built.

use crate::domain::errors::DomainError;

/// Headers that would change message framing or connection semantics and
/// therefore cannot be supplied by the caller.
const FORBIDDEN_HEADER_NAMES: [&str; 10] = [
    "Content-Length",
    "Transfer-Encoding",
    "Connection",
    "Keep-Alive",
    "Proxy-Connection",
    "TE",
    "Trailer",
    "Upgrade",
    "Proxy-Authenticate",
    "Proxy-Authorization",
];

/// A parsed and validated custom request header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomHeader {
    pub name: String,
    pub value: String,
}

impl CustomHeader {
    /// Parses one `Name: value` occurrence.
    ///
    /// Only the first `:` separates the name; later colons belong to the
    /// value (so URLs and other colon-bearing values work unchanged). HTTP
    /// optional whitespace (ASCII space and tab) is trimmed from both ends of
    /// the name and value. Values are restricted to tab and printable ASCII.
    /// Errors never echo the value; the header name appears only when the name
    /// itself is valid.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let (name_raw, value_raw) = raw.split_once(':').ok_or_else(|| {
            header_error(
                "expected a 'Name: value' pair (the name and value must be separated by ':')",
            )
        })?;
        let name = trim_ows(name_raw);
        let value = trim_ows(value_raw);

        if name.is_empty() {
            return Err(header_error("header name must not be empty"));
        }
        if !name.bytes().all(is_token_byte) {
            return Err(header_error(
                "header name contains a character that is not valid in an HTTP token",
            ));
        }
        if is_forbidden_header_name(name) {
            return Err(header_error(format!("header '{}' is not allowed", name)));
        }
        if let Some(byte) = value
            .bytes()
            .find(|&byte| byte != b'\t' && !(0x20..=0x7E).contains(&byte))
        {
            return Err(header_error(format!(
                "value for header '{}' contains a control or non-ASCII byte (0x{byte:02x})",
                name
            )));
        }

        Ok(Self {
            name: name.to_owned(),
            value: value.to_owned(),
        })
    }
}

/// Returns whether `name` is on the framing/hop-by-hop denylist. Comparison is
/// ASCII case-insensitive. Exposed so request construction can re-check names
/// as defense in depth.
pub fn is_forbidden_header_name(name: &str) -> bool {
    FORBIDDEN_HEADER_NAMES
        .iter()
        .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
}

/// Trims HTTP optional whitespace (SP / HTAB) from both ends only.
fn trim_ows(value: &str) -> &str {
    value.trim_matches([' ', '\t'])
}

/// RFC 9110 `token` character.
fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

fn header_error(message: impl Into<String>) -> DomainError {
    DomainError::Header {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_headers() {
        let header = CustomHeader::parse("X-Custom: value").unwrap();
        assert_eq!(
            header,
            CustomHeader {
                name: "X-Custom".to_owned(),
                value: "value".to_owned(),
            }
        );

        let header = CustomHeader::parse("Authorization: Bearer abc").unwrap();
        assert_eq!(header.name, "Authorization");
        assert_eq!(header.value, "Bearer abc");
    }

    #[test]
    fn keeps_colons_inside_value() {
        let header = CustomHeader::parse("X-Url: https://example.com/a:b").unwrap();
        assert_eq!(header.name, "X-Url");
        assert_eq!(header.value, "https://example.com/a:b");
    }

    #[test]
    fn trims_only_optional_whitespace() {
        let header = CustomHeader::parse(" \tX-Padded \t:\t value\t ").unwrap();
        assert_eq!(header.name, "X-Padded");
        assert_eq!(header.value, "value");

        let header = CustomHeader::parse("X-Space: a  b").unwrap();
        assert_eq!(header.value, "a  b");

        let header = CustomHeader::parse("X-Tab: a\tb").unwrap();
        assert_eq!(header.value, "a\tb");
    }

    #[test]
    fn accepts_empty_value() {
        let header = CustomHeader::parse("X-Empty:").unwrap();
        assert_eq!(header.name, "X-Empty");
        assert_eq!(header.value, "");
    }

    #[test]
    fn accepts_token_names_and_headers_that_are_not_denylisted() {
        for name in [
            "Content-Type",
            "Content-Encoding",
            "Authorization",
            "Host",
            "Cookie",
            "X_Under",
            "x-token!#$%&'*+-.^`|~",
        ] {
            let raw = format!("{name}: value");
            let header = CustomHeader::parse(&raw).unwrap();
            assert_eq!(header.name, name, "{raw}");
        }
    }

    #[test]
    fn missing_colon_is_rejected() {
        let err = CustomHeader::parse("X-Custom value").unwrap_err();
        assert!(matches!(err, DomainError::Header { .. }), "{err}");
        assert!(err.to_string().starts_with("invalid header:"), "{err}");
    }

    #[test]
    fn empty_name_is_rejected() {
        for raw in [": value", "   : value", "\t: value"] {
            let err = CustomHeader::parse(raw).unwrap_err();
            assert!(matches!(err, DomainError::Header { .. }), "{raw}: {err}");
            assert!(err.to_string().contains("must not be empty"), "{err}");
        }
    }

    #[test]
    fn invalid_name_characters_are_rejected() {
        for raw in [
            "Bad Name: value",
            "Bad@Name: value",
            "Bad/Name: value",
            "Bad(Name): value",
            "BadéName: value",
            "Bad\rName: value",
            "Bad\nName: value",
            "Bad\0Name: value",
        ] {
            let err = CustomHeader::parse(raw).unwrap_err();
            assert!(matches!(err, DomainError::Header { .. }), "{raw:?}: {err}");
            assert!(
                err.to_string().contains("HTTP token") || err.to_string().contains("empty"),
                "{raw:?}: {err}"
            );
        }
    }

    #[test]
    fn invalid_value_bytes_are_rejected() {
        for value in [
            "ab\u{0}cd",
            "ab\u{1}cd",
            "ab\rcd",
            "ab\ncd",
            "ab\u{7f}cd",
            "caf\u{e9}",
            "\u{80}",
        ] {
            let raw = format!("X-Test: {value}");
            let err = CustomHeader::parse(&raw).unwrap_err();
            assert!(matches!(err, DomainError::Header { .. }), "{raw:?}: {err}");
            assert!(!err.to_string().contains(value), "{raw:?}: {err}");
        }
    }

    #[test]
    fn every_denylisted_name_is_rejected_in_any_case() {
        for name in FORBIDDEN_HEADER_NAMES {
            for variant in [
                name.to_owned(),
                name.to_ascii_lowercase(),
                name.to_ascii_uppercase(),
            ] {
                let raw = format!("{variant}: value");
                let err = CustomHeader::parse(&raw).unwrap_err();
                assert!(matches!(err, DomainError::Header { .. }), "{raw}: {err}");
                assert!(err.to_string().contains("not allowed"), "{raw}: {err}");
            }
        }
    }

    #[test]
    fn denylist_predicate_is_case_insensitive() {
        assert!(is_forbidden_header_name("content-length"));
        assert!(is_forbidden_header_name("CONNECTION"));
        assert!(is_forbidden_header_name("Te"));
        assert!(!is_forbidden_header_name("Content-Type"));
        assert!(!is_forbidden_header_name("Authorization"));
    }

    #[test]
    fn errors_never_include_the_value() {
        let secret = "s3cr3t-value";
        let err = CustomHeader::parse(&format!("X-Test: {secret}\r\nX-Evil: 1")).unwrap_err();
        assert!(!err.to_string().contains(secret), "{err}");
        assert!(!err.to_string().contains("X-Evil"), "{err}");
    }
}
