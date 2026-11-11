//! Pure multipart/form-data support: form-part parsing, per-part content-type
//! resolution, boundary generation, and the byte-exact framing encoder.
//!
//! The domain owns the wire format so request construction stays pure and
//! byte-testable. File reading happens in infrastructure, before an invocation
//! is built; this module only ever sees materialized bytes.
//!
//! Out of scope (deliberately): streaming, boundary/content collision
//! renegotiation, per-part `encoding.headers` such as `Content-ID`, and
//! escaping field names or filenames (unsafe characters are rejected instead).

use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::errors::DomainError;
use crate::domain::model::{PartKind, PartSpec};

/// One `-F <field>=<value>` occurrence before file bytes are materialized.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormInput {
    pub name: String,
    pub value: FormInputValue,
}

/// The two input syntaxes: a plain text value or a file path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormInputValue {
    Text(String),
    File {
        path: String,
        content_type: Option<String>,
    },
}

impl FormInput {
    /// Parses one `field=value` or `field=@path[;type=<content-type>]`
    /// occurrence.
    ///
    /// Field names and filenames must not contain `"`, `\`, CR, or LF because
    /// they are interpolated into `Content-Disposition`. Text values are
    /// otherwise unconstrained (an empty value frames an empty part). A
    /// `;type=` postfix is valid only on file inputs.
    pub fn parse(raw: &str) -> Result<Self, DomainError> {
        let (name, value) = raw.split_once('=').ok_or_else(|| {
            form_error("expected '<field>=<value>' (the field and value must be separated by '=')")
        })?;
        validate_field_name(name)?;

        if let Some(rest) = value.strip_prefix('@') {
            let (path, content_type) = match rest.split_once(";type=") {
                Some((path, content_type)) => (path, Some(content_type)),
                None => (rest, None),
            };
            if path.is_empty() {
                return Err(form_error(format!(
                    "field '{name}' must name a file path after '@'"
                )));
            }
            if let Some(content_type) = content_type {
                validate_content_type(content_type)?;
            }
            Ok(Self {
                name: name.to_owned(),
                value: FormInputValue::File {
                    path: path.to_owned(),
                    content_type: content_type.map(str::to_owned),
                },
            })
        } else {
            if value.contains(";type=") {
                return Err(form_error(format!(
                    "';type=' is valid only on file parts (field '{name}')"
                )));
            }
            Ok(Self {
                name: name.to_owned(),
                value: FormInputValue::Text(value.to_owned()),
            })
        }
    }
}

/// A form part with file bytes already read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormPart {
    pub name: String,
    pub value: FormPartValue,
}

/// Materialized part payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormPartValue {
    Text(String),
    File {
        filename: String,
        bytes: Vec<u8>,
        content_type: Option<String>,
    },
}

impl FormPartValue {
    /// Framing kind follows the input syntax, never the spec.
    pub fn input_kind(&self) -> PartKind {
        match self {
            Self::Text(_) => PartKind::Text,
            Self::File { .. } => PartKind::File,
        }
    }
}

/// Resolves a part's `Content-Type` using the documented precedence chain:
/// caller `;type=` > captured `encoding.contentType` > inference from the
/// part's schema kind > input-syntax default.
///
/// When a matching `PartSpec` exists, its kind is authoritative for inference,
/// so a documented text part stays untyped on the wire (servers read that as
/// `text/plain`) even when the caller supplies a file path.
pub fn resolve_part_content_type(
    input_kind: PartKind,
    caller_content_type: Option<&str>,
    spec: Option<&PartSpec>,
) -> Option<String> {
    if let Some(content_type) = caller_content_type {
        return Some(content_type.to_owned());
    }
    if let Some(spec) = spec {
        if let Some(content_type) = &spec.content_type {
            return Some(content_type.clone());
        }
        return match spec.kind {
            PartKind::File => Some("application/octet-stream".to_owned()),
            PartKind::Object => Some("application/json".to_owned()),
            PartKind::Text => None,
        };
    }
    match input_kind {
        PartKind::File => Some("application/octet-stream".to_owned()),
        PartKind::Text | PartKind::Object => None,
    }
}

/// Encodes materialized parts with an explicit boundary.
///
/// Framing is exactly RFC 7578 style: a boundary delimiter, the
/// `Content-Disposition` header (plus `filename` for file inputs), an optional
/// `Content-Type` header, a blank line, the raw part bytes, then a trailing
/// CRLF. The final `--<boundary>--\r\n` terminator is always appended. Part
/// order and repeated field names are preserved.
pub fn encode(boundary: &str, parts: &[FormPart], specs: &[PartSpec]) -> Vec<u8> {
    let mut out = Vec::new();
    for part in parts {
        push_part(&mut out, boundary, part, specs);
    }
    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"--\r\n");
    out
}

fn push_part(out: &mut Vec<u8>, boundary: &str, part: &FormPart, specs: &[PartSpec]) {
    let spec = specs.iter().find(|spec| spec.name == part.name);
    let (filename, caller_content_type, bytes) = match &part.value {
        FormPartValue::Text(value) => (None, None, value.as_bytes()),
        FormPartValue::File {
            filename,
            bytes,
            content_type,
        } => (
            Some(filename.as_str()),
            content_type.as_deref(),
            bytes.as_slice(),
        ),
    };

    out.extend_from_slice(b"--");
    out.extend_from_slice(boundary.as_bytes());
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(b"Content-Disposition: form-data; name=\"");
    out.extend_from_slice(part.name.as_bytes());
    out.extend_from_slice(b"\"");
    if let Some(filename) = filename {
        out.extend_from_slice(b"; filename=\"");
        out.extend_from_slice(filename.as_bytes());
        out.extend_from_slice(b"\"");
    }
    out.extend_from_slice(b"\r\n");

    if let Some(content_type) =
        resolve_part_content_type(part.value.input_kind(), caller_content_type, spec)
    {
        out.extend_from_slice(b"Content-Type: ");
        out.extend_from_slice(content_type.as_bytes());
        out.extend_from_slice(b"\r\n");
    }

    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(bytes);
    out.extend_from_slice(b"\r\n");
}

/// Generates a fresh boundary per request: `clining-` plus 32 ASCII hex
/// characters mixed from the current time, the process id, and a counter.
pub fn boundary() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos() as u64)
        .unwrap_or(0);
    let pid = u64::from(process::id());
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = splitmix64(nanos ^ splitmix64(pid) ^ splitmix64(counter));
    format!("clining-{seed:016x}{:016x}", splitmix64(seed ^ counter))
}

fn splitmix64(mut value: u64) -> u64 {
    value = value.wrapping_add(0x9E37_79B9_7F4A_7C15);
    value = (value ^ (value >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

/// Whether a media type is `multipart/form-data` (exact subtype match, ASCII
/// case-insensitive, parameters after the first `;` ignored).
pub fn is_multipart_form_data(content_type: &str) -> bool {
    let media_type = content_type
        .split_once(';')
        .map_or(content_type, |(media_type, _)| media_type)
        .trim();
    media_type.eq_ignore_ascii_case("multipart/form-data")
}

/// Validates a field name before it is interpolated into framing.
pub fn validate_field_name(name: &str) -> Result<(), DomainError> {
    if name.is_empty() {
        return Err(form_error("field name must not be empty"));
    }
    if contains_unsafe_framing_text(name) {
        return Err(form_error(
            "field name contains a quote, backslash, or line break, which cannot be safely framed",
        ));
    }
    Ok(())
}

/// Validates a filename before it is interpolated into `Content-Disposition`.
pub fn validate_filename(filename: &str) -> Result<(), DomainError> {
    if filename.is_empty() {
        return Err(form_error("filename must not be empty"));
    }
    if contains_unsafe_framing_text(filename) {
        return Err(form_error(
            "filename contains a quote, backslash, or line break, which cannot be safely framed",
        ));
    }
    Ok(())
}

fn validate_content_type(content_type: &str) -> Result<(), DomainError> {
    if content_type.is_empty() {
        return Err(form_error("content type after ';type=' must not be empty"));
    }
    if content_type.contains('"') || content_type.contains('\r') || content_type.contains('\n') {
        return Err(form_error(
            "content type contains a quote or line break, which cannot be safely framed",
        ));
    }
    Ok(())
}

fn contains_unsafe_framing_text(value: &str) -> bool {
    value.contains('"') || value.contains('\\') || value.contains('\r') || value.contains('\n')
}

fn form_error(message: impl Into<String>) -> DomainError {
    DomainError::Form {
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_part(name: &str, value: &str) -> FormPart {
        FormPart {
            name: name.to_owned(),
            value: FormPartValue::Text(value.to_owned()),
        }
    }

    fn file_part(name: &str, filename: &str, bytes: &[u8]) -> FormPart {
        FormPart {
            name: name.to_owned(),
            value: FormPartValue::File {
                filename: filename.to_owned(),
                bytes: bytes.to_vec(),
                content_type: None,
            },
        }
    }

    fn spec(name: &str, kind: PartKind, content_type: Option<&str>) -> PartSpec {
        PartSpec {
            name: name.to_owned(),
            required: false,
            kind,
            repeated: false,
            content_type: content_type.map(str::to_owned),
        }
    }

    #[test]
    fn frames_parts_with_exact_crlf_layout_and_order() {
        let parts = vec![
            text_part("name", "fluffy"),
            file_part("photo", "cat.png", b"PNG"),
        ];
        let specs = vec![spec("photo", PartKind::File, Some("image/png"))];
        let encoded = encode("fixed", &parts, &specs);
        assert_eq!(
            encoded,
            b"--fixed\r\n\
              Content-Disposition: form-data; name=\"name\"\r\n\
              \r\n\
              fluffy\r\n\
              --fixed\r\n\
              Content-Disposition: form-data; name=\"photo\"; filename=\"cat.png\"\r\n\
              Content-Type: image/png\r\n\
              \r\n\
              PNG\r\n\
              --fixed--\r\n"
                .to_vec()
        );
    }

    #[test]
    fn preserves_repeated_field_names_as_separate_parts() {
        let parts = vec![text_part("name", "a"), text_part("name", "b")];
        let encoded = encode("fixed", &parts, &[]);
        assert_eq!(count_occurrences(&encoded, b"name=\"name\""), 2);
        let first = position(&encoded, b"a\r\n--fixed").expect("first part");
        let second = position(&encoded, b"b\r\n--fixed").expect("second part");
        assert!(first < second, "input order is preserved");
    }

    #[test]
    fn file_bytes_survive_verbatim_including_binary_and_boundary_text() {
        let bytes = b"before--fixed\r\n\x00\xffafter";
        let encoded = encode("fixed", &[file_part("blob", "b.bin", bytes)], &[]);
        let marker = position(&encoded, b"\r\n\r\n").expect("header terminator");
        let start = marker + 4;
        assert_eq!(&encoded[start..start + bytes.len()], bytes);
    }

    #[test]
    fn terminator_is_always_present_even_with_no_parts() {
        assert_eq!(encode("fixed", &[], &[]), b"--fixed--\r\n".to_vec());
    }

    fn position(haystack: &[u8], needle: &[u8]) -> Option<usize> {
        haystack.windows(needle.len()).position(|w| w == needle)
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        haystack
            .windows(needle.len())
            .filter(|window| *window == needle)
            .count()
    }

    #[test]
    fn parses_text_and_file_inputs() {
        assert_eq!(
            FormInput::parse("name=fluffy").unwrap(),
            FormInput {
                name: "name".to_owned(),
                value: FormInputValue::Text("fluffy".to_owned()),
            }
        );
        assert_eq!(
            FormInput::parse("name=").unwrap(),
            FormInput {
                name: "name".to_owned(),
                value: FormInputValue::Text(String::new()),
            }
        );
        assert_eq!(
            FormInput::parse("photo=@/tmp/cat.png").unwrap(),
            FormInput {
                name: "photo".to_owned(),
                value: FormInputValue::File {
                    path: "/tmp/cat.png".to_owned(),
                    content_type: None,
                },
            }
        );
        assert_eq!(
            FormInput::parse("photo=@/tmp/cat.png;type=image/png").unwrap(),
            FormInput {
                name: "photo".to_owned(),
                value: FormInputValue::File {
                    path: "/tmp/cat.png".to_owned(),
                    content_type: Some("image/png".to_owned()),
                },
            }
        );
        assert_eq!(
            FormInput::parse("url=https://example.com/a=b").unwrap(),
            FormInput {
                name: "url".to_owned(),
                value: FormInputValue::Text("https://example.com/a=b".to_owned()),
            }
        );
    }

    #[test]
    fn rejects_malformed_inputs_as_single_line_form_errors() {
        for raw in [
            "name",
            "=value",
            "photo=@",
            "photo=@;type=image/png",
            "name=value;type=text/plain",
            "na\"me=value",
            "na\\me=value",
            "na\rme=value",
            "na\nme=value",
            "photo=@/tmp/cat.png;type=",
            "photo=@/tmp/cat.png;type=a\"b",
            "photo=@/tmp/cat.png;type=a\rb",
        ] {
            let err = FormInput::parse(raw).unwrap_err();
            assert!(matches!(err, DomainError::Form { .. }), "{raw:?}: {err}");
            assert_eq!(err.to_string().lines().count(), 1, "{raw:?}: {err}");
            assert!(
                err.to_string().starts_with("invalid form part:"),
                "{raw:?}: {err}"
            );
        }
    }

    #[test]
    fn rejects_unsafe_filenames() {
        for filename in ["a\"b", "a\\b", "a\rb", "a\nb"] {
            let err = validate_filename(filename).unwrap_err();
            assert!(matches!(err, DomainError::Form { .. }), "{filename:?}");
            assert_eq!(err.to_string().lines().count(), 1, "{filename:?}: {err}");
        }
        assert!(validate_filename("cat.png").is_ok());
    }

    #[test]
    fn caller_type_wins_over_everything() {
        let part = spec("photo", PartKind::File, Some("image/png"));
        assert_eq!(
            resolve_part_content_type(PartKind::File, Some("application/x-custom"), Some(&part)),
            Some("application/x-custom".to_owned())
        );
    }

    #[test]
    fn captured_encoding_wins_over_inference() {
        let part = spec("photo", PartKind::File, Some("image/jpeg"));
        assert_eq!(
            resolve_part_content_type(PartKind::File, None, Some(&part)),
            Some("image/jpeg".to_owned())
        );
    }

    #[test]
    fn infers_content_type_from_spec_kind() {
        assert_eq!(
            resolve_part_content_type(
                PartKind::File,
                None,
                Some(&spec("photo", PartKind::File, None))
            ),
            Some("application/octet-stream".to_owned())
        );
        assert_eq!(
            resolve_part_content_type(
                PartKind::File,
                None,
                Some(&spec("meta", PartKind::Object, None))
            ),
            Some("application/json".to_owned())
        );
        assert_eq!(
            resolve_part_content_type(
                PartKind::File,
                None,
                Some(&spec("name", PartKind::Text, None))
            ),
            None
        );
    }

    #[test]
    fn falls_back_to_input_syntax_defaults_without_spec_metadata() {
        assert_eq!(
            resolve_part_content_type(PartKind::File, None, None),
            Some("application/octet-stream".to_owned())
        );
        assert_eq!(resolve_part_content_type(PartKind::Text, None, None), None);
    }

    #[test]
    fn text_parts_get_no_content_type_unless_declared() {
        let encoded = encode("fixed", &[text_part("name", "x")], &[]);
        assert!(!contains(&encoded, b"Content-Type:"));
        let declared = spec("name", PartKind::Text, Some("text/plain; charset=utf-8"));
        let encoded = encode("fixed", &[text_part("name", "x")], &[declared]);
        assert!(contains(
            &encoded,
            b"Content-Type: text/plain; charset=utf-8"
        ));
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        position(haystack, needle).is_some()
    }

    #[test]
    fn boundary_has_the_expected_shape_and_changes_per_call() {
        let first = boundary();
        std::thread::sleep(std::time::Duration::from_millis(1));
        let second = boundary();
        assert!(first.starts_with("clining-"), "{first}");
        assert!(first.len() >= 24, "{first}");
        assert!(
            first["clining-".len()..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit()),
            "{first}"
        );
        assert_ne!(first, second);
    }

    #[test]
    fn classifies_multipart_form_data_only() {
        assert!(is_multipart_form_data("multipart/form-data"));
        assert!(is_multipart_form_data("Multipart/Form-Data; charset=utf-8"));
        assert!(is_multipart_form_data(" MULTIPART/FORM-DATA ; boundary=x"));
        assert!(!is_multipart_form_data("multipart/mixed"));
        assert!(!is_multipart_form_data("application/json"));
    }
}
