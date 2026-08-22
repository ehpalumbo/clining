//! Serde structs for the OpenAPI 3.0.x subset.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct OpenApi30Spec {
    pub info: Option<Info>,
    pub servers: Option<Vec<Server>>,
    pub paths: BTreeMap<String, PathItem>,
    pub tags: Option<Vec<Tag>>,
    pub components: Option<Components>,
}

/// Reusable component objects; only `schemas` are used today.
#[derive(Debug, Deserialize)]
pub struct Components {
    pub schemas: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
pub struct Info {
    pub title: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Server {
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct Tag {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub struct PathItem {
    pub get: Option<Operation>,
    pub put: Option<Operation>,
    pub post: Option<Operation>,
    pub delete: Option<Operation>,
    pub patch: Option<Operation>,
    pub head: Option<Operation>,
    pub options: Option<Operation>,
    pub trace: Option<Operation>,
    pub parameters: Option<Vec<Parameter>>,
}

#[derive(Debug, Deserialize, Default)]
pub struct Operation {
    #[serde(rename = "operationId")]
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub tags: Option<Vec<String>>,
    pub parameters: Option<Vec<Parameter>>,
    #[serde(rename = "requestBody")]
    pub request_body: Option<RequestBody>,
    pub responses: Option<BTreeMap<String, Response>>,
}

/// A response object; only body content is captured.
#[derive(Debug, Deserialize)]
pub struct Response {
    pub content: Option<BTreeMap<String, MediaType>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Parameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: String,
    pub required: Option<bool>,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RequestBody {
    pub required: Option<bool>,
    pub content: BTreeMap<String, MediaType>,
}

#[derive(Debug, Deserialize)]
pub struct MediaType {
    pub schema: Option<serde_json::Value>,
}
