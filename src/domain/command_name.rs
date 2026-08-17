//! Pure command and parameter naming rules (Phase 2).

use std::collections::{HashMap, HashSet};

use crate::domain::model::HttpMethod;

/// Command name for an operation: kebab-cased `operationId` when present,
/// otherwise a method + path-segments fallback.
pub fn command_name(operation_id: Option<&str>, method: HttpMethod, path: &str) -> String {
    match operation_id {
        Some(id) if !id.trim().is_empty() => kebab_case(id),
        _ => fallback_name(method, path),
    }
}

/// CLI name for a parameter: kebab-cased from the spec name.
pub fn cli_name(param_name: &str) -> String {
    kebab_case(param_name)
}

/// Disambiguates duplicate names within a group with numeric suffixes (`-2`, `-3`, …).
pub fn disambiguate(names: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut used: HashSet<String> = HashSet::new();
    let mut occurrences: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for name in names {
        let count = occurrences.entry(name.clone()).or_insert(0);
        *count += 1;
        let mut candidate = if *count == 1 {
            name.clone()
        } else {
            format!("{name}-{count}")
        };
        while used.contains(&candidate) {
            *count += 1;
            candidate = format!("{name}-{count}");
        }
        used.insert(candidate.clone());
        out.push(candidate);
    }
    out
}

fn fallback_name(method: HttpMethod, path: &str) -> String {
    let mut parts: Vec<&str> = vec![method.as_str()];
    for segment in path.split('/') {
        if !segment.is_empty() && !segment.starts_with('{') {
            parts.push(segment);
        }
    }
    kebab_case(&parts.join("-"))
}

/// Converts CamelCase / snake_case / kebab input to kebab-case.
fn kebab_case(input: &str) -> String {
    let chars: Vec<char> = input.chars().collect();
    let mut out = String::new();
    let mut prev: Option<char> = None;
    for (i, c) in chars.iter().copied().enumerate() {
        if matches!(c, '_' | '-' | ' ' | '.') {
            if prev.is_some() {
                out.push('-');
                prev = None;
            }
            continue;
        }
        if c.is_ascii_uppercase() {
            let after_boundary = prev.is_some_and(|p| p.is_lowercase() || p.is_ascii_digit());
            let before_lower = chars.get(i + 1).is_some_and(|n| n.is_lowercase());
            if prev.is_some() && (after_boundary || before_lower) {
                out.push('-');
            }
        }
        out.push(c.to_ascii_lowercase());
        prev = Some(c);
    }
    out.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kebab_cases_operation_id() {
        assert_eq!(
            command_name(Some("getPetById"), HttpMethod::Get, "/pets/{petId}"),
            "get-pet-by-id"
        );
    }

    #[test]
    fn fallback_from_method_and_path() {
        assert_eq!(
            command_name(None, HttpMethod::Get, "/pets/{petId}"),
            "get-pets"
        );
        assert_eq!(
            command_name(None, HttpMethod::Post, "/store/orders"),
            "post-store-orders"
        );
    }

    #[test]
    fn empty_operation_id_falls_back() {
        assert_eq!(
            command_name(Some(""), HttpMethod::Post, "/orders"),
            "post-orders"
        );
    }

    #[test]
    fn cli_names_are_kebab_cased() {
        assert_eq!(cli_name("petStatus"), "pet-status");
        assert_eq!(cli_name("already-kebab"), "already-kebab");
        assert_eq!(cli_name("XRequestID"), "x-request-id");
    }

    #[test]
    fn disambiguates_duplicates() {
        let names = vec![
            "get-pets".to_owned(),
            "get-pets".to_owned(),
            "place".to_owned(),
        ];
        assert_eq!(
            disambiguate(names),
            vec![
                "get-pets".to_owned(),
                "get-pets-2".to_owned(),
                "place".to_owned()
            ]
        );
    }

    #[test]
    fn disambiguate_skips_names_that_already_end_in_suffix() {
        let names = vec![
            "get-pets".to_owned(),
            "get-pets-2".to_owned(),
            "get-pets".to_owned(),
        ];
        assert_eq!(
            disambiguate(names),
            vec![
                "get-pets".to_owned(),
                "get-pets-2".to_owned(),
                "get-pets-3".to_owned(),
            ]
        );
    }
}
