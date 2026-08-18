//! Domain error vocabulary: not found, invalid stored model, invalid spec,
//! parameter/body errors, network, and I/O.

use thiserror::Error;

/// Errors produced by the domain ports and use cases.
#[derive(Debug, Error)]
pub enum DomainError {
    #[error("no API installed under the name '{name}' (looked in '{path}')")]
    NotFound { name: String, path: String },

    #[error("stored model for '{name}' at '{path}' is invalid: {reason}")]
    InvalidStoredModel {
        name: String,
        path: String,
        reason: String,
    },

    #[error("invalid OpenAPI spec: {message}")]
    InvalidSpec { message: String },

    #[error("invalid name '{name}': {reason}")]
    InvalidName { name: String, reason: String },

    #[error("parameter error: {message}")]
    Parameter { message: String },

    #[error("body error: {message}")]
    Body { message: String },

    #[error("network error: {message}")]
    Network { message: String },

    #[error("I/O error: {message}")]
    Io { message: String },
}
