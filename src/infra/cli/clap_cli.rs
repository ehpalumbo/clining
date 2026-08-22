//! clap CLI: a single builder-API command tree — a static `install` subcommand
//! plus a per-API subcommand tree added once the installed model is loaded.

use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};

use crate::application::invoke_operation::InvokeOperationService;
use crate::application::learn_api::LearnApiService;
use crate::domain::errors::DomainError;
use crate::domain::model::{
    ApiInvocationRequest, ApiModel, ApiOperation, ApiOperationGroup, BodySpec, Param, SchemaSpec,
};
use crate::domain::ports::{ApiStore, HttpInvoker, OpenApiParser, SpecLoader};

/// Builds the full CLI command tree: the `install` subcommand plus, when a
/// model is provided, the installed API as a nested subcommand tree. Without a
/// model, a placeholder `<api>` entry keeps the invocation shape visible in
/// `clining --help`.
pub fn build_cli_command(model: Option<&ApiModel>) -> Command {
    let top = Command::new("clining")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Expose OpenAPI-documented HTTP APIs as local CLI commands")
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .subcommand(install_command());
    match model {
        Some(model) => top.subcommand(api_command(model)),
        None => top.subcommand(api_placeholder_command()),
    }
}

/// Builds the static `install` subcommand with the builder API.
fn install_command() -> Command {
    Command::new("install")
        .about("Install an API from an OpenAPI 3.0 spec.")
        .disable_help_subcommand(true)
        .arg(
            Arg::new("name")
                .value_name("name")
                .required(true)
                .help("Name under which to store the API model."),
        )
        .arg(
            Arg::new("spec_source")
                .value_name("spec")
                .required(true)
                .help("Path to local file or http(s) URL of an OpenAPI 3.0 JSON document."),
        )
        .arg(
            Arg::new("base_url")
                .long("base-url")
                .help("Override the base URL taken from servers[0].url."),
        )
}

/// Stand-in for the dynamic per-API subcommand, shown when no model is loaded.
/// The static path only parses flags, `install`, or no arguments, so the
/// placeholder is never actually matched.
fn api_placeholder_command() -> Command {
    Command::new("<api>").about("Commands for an installed API: clining <name> <group> <command>")
}

/// Builds the dynamic per-API subcommand tree: `<group>` subcommands each
/// containing `<command>` subcommands with per-parameter `--long` args.
/// Every level of `--help` renders useful, spec-derived information.
fn api_command(model: &ApiModel) -> Command {
    let mut api = Command::new(model.name.clone())
        .about(format!("Commands for API '{}'", model.name))
        .disable_help_subcommand(true)
        .subcommand_required(true);
    for group in &model.operation_groups {
        let mut group_cmd = Command::new(group.name.clone())
            .subcommand_required(true)
            .about(group_about(group));
        for operation in &group.operations {
            let mut command_cmd = Command::new(operation.name.clone());
            if let Some(summary) = &operation.summary {
                command_cmd = command_cmd.about(summary);
            }
            for param in &operation.path_params {
                command_cmd = command_cmd.arg(
                    Arg::new(param.canonical_name.clone())
                        .long(param.canonical_name.clone())
                        .value_name(param.name.clone())
                        .required(true)
                        .help(param_help_text(param)),
                );
            }
            for param in &operation.query_params {
                let mut arg = Arg::new(param.canonical_name.clone())
                    .long(param.canonical_name.clone())
                    .value_name(param.name.clone())
                    .action(ArgAction::Append)
                    .num_args(1..)
                    .help(param_help_text(param));
                if param.required {
                    arg = arg.required(true);
                }
                command_cmd = command_cmd.arg(arg);
            }
            command_cmd = command_cmd.after_help(command_footer(model, operation));
            group_cmd = group_cmd.subcommand(command_cmd);
        }
        api = api.subcommand(group_cmd);
    }
    api
}

/// Help text for a group subcommand: the tag description when present, else a
/// command-count summary.
fn group_about(group: &ApiOperationGroup) -> String {
    match &group.description {
        Some(description) => description.clone(),
        None => format!("{} commands", group.operations.len()),
    }
}

/// Help text for a parameter argument: its description plus a required marker.
fn param_help_text(param: &Param) -> String {
    let mut text = param.description.clone().unwrap_or_default();
    if param.required {
        if !text.is_empty() {
            text.push(' ');
        }
        text.push_str("[required]");
    }
    text
}

/// Footer shown under a command's `--help`: the request line and, when the
/// operation declares a body, its content type, requiredness, and a
/// fully-resolved JSON-schema tree.
fn command_footer(model: &ApiModel, command: &ApiOperation) -> String {
    let mut text = format!("Request: {} {}", command.method.as_str(), command.path);
    if let Some(body) = &command.request_body {
        text.push_str(&format!(
            "\nBody: {} ({})",
            body.content_type,
            body_requiredness(body)
        ));
        text.push_str(&format!(
            ", schema: {}\n",
            render_body_schema(model, &body.schema)
        ));
    }
    text
}

fn body_requiredness(body: &BodySpec) -> &'static str {
    if body.required {
        "required"
    } else {
        "optional"
    }
}

/// Renders the operation's request body schema as compact JSON, fully
/// expanding registry refs. `None` renders as the literal `unknown`.
fn render_body_schema(model: &ApiModel, schema: &Option<SchemaSpec>) -> String {
    match schema {
        Some(spec) => serde_json::to_string(&render_schema(spec, model, &mut Vec::new()))
            .unwrap_or_else(|_| "{}".to_owned()),
        None => "unknown".to_owned(),
    }
}

/// Renders a `SchemaSpec` as a JSON-schema value, expanding registry refs
/// recursively. `seen` is a path stack used to terminate cycles: a ref already
/// on the stack (or absent from the registry) renders as a `$ref` marker.
fn render_schema(spec: &SchemaSpec, model: &ApiModel, seen: &mut Vec<String>) -> serde_json::Value {
    match spec {
        SchemaSpec::Ref { ref_id } => {
            if seen.iter().any(|id| id == ref_id) {
                return serde_json::json!({ "$ref": format!("#/components/schemas/{ref_id}") });
            }
            match model.schema_by_ref_id(ref_id) {
                Some(target) => {
                    seen.push(ref_id.clone());
                    let rendered = render_schema(target, model, seen);
                    seen.pop();
                    rendered
                }
                None => serde_json::json!({ "$ref": format!("#/components/schemas/{ref_id}") }),
            }
        }
        SchemaSpec::Object {
            properties,
            extra_json,
        } => {
            let mut map = extra_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .and_then(|v| match v {
                    serde_json::Value::Object(m) => Some(m),
                    _ => None,
                })
                .unwrap_or_default();
            if !map.contains_key("type") {
                map.insert("type".to_owned(), serde_json::json!("object"));
            }
            if !properties.is_empty() {
                let mut props = serde_json::Map::new();
                for (name, prop) in properties {
                    props.insert(name.clone(), render_schema(prop, model, seen));
                }
                map.insert("properties".to_owned(), serde_json::Value::Object(props));
            }
            serde_json::Value::Object(map)
        }
        SchemaSpec::Array { items, extra_json } => {
            let mut map = extra_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .and_then(|v| match v {
                    serde_json::Value::Object(m) => Some(m),
                    _ => None,
                })
                .unwrap_or_default();
            if !map.contains_key("type") {
                map.insert("type".to_owned(), serde_json::json!("array"));
            }
            if let Some(items) = items {
                map.insert("items".to_owned(), render_schema(items, model, seen));
            }
            serde_json::Value::Object(map)
        }
        SchemaSpec::Primitive { raw_json } => {
            serde_json::from_str(raw_json).unwrap_or_else(|_| serde_json::json!({}))
        }
        SchemaSpec::Composite {
            composite_kind,
            schemas,
            extra_json,
        } => {
            let mut map = extra_json
                .as_deref()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .and_then(|v| match v {
                    serde_json::Value::Object(m) => Some(m),
                    _ => None,
                })
                .unwrap_or_default();
            let rendered: Vec<serde_json::Value> = schemas
                .iter()
                .map(|schema| render_schema(schema, model, seen))
                .collect();
            map.insert(
                composite_kind.keyword().to_owned(),
                serde_json::Value::Array(rendered),
            );
            serde_json::Value::Object(map)
        }
        SchemaSpec::Unknown { raw_json } => {
            serde_json::from_str(raw_json).unwrap_or_else(|_| serde_json::json!({}))
        }
    }
}

/// Top-level dispatch decision based on the first positional argument.
enum Action {
    Install(Vec<String>),
    Invoke(Vec<String>),
    Static(Vec<String>),
}

/// Dispatches the CLI arguments to the appropriate action: `install` subcommand,
/// dynamic per-API command tree, or static help/test tree.
fn dispatch(argv: &[String]) -> Action {
    match argv.get(1).map(String::as_str) {
        Some("install") => Action::Install(argv.to_vec()),
        Some(first) if !first.starts_with('-') => Action::Invoke(argv.to_vec()),
        _ => Action::Static(argv.to_vec()),
    }
}

/// Main CLI application, holding the services and store needed to run commands.
pub struct CliApp<'a, L, P, S, I>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
    I: HttpInvoker,
{
    learn: LearnApiService<'a, L, P, S>,
    store: &'a S,
    invoker: I,
}

impl<'a, L, P, S, I> CliApp<'a, L, P, S, I>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
    I: HttpInvoker,
{
    pub fn new(learn: LearnApiService<'a, L, P, S>, store: &'a S, invoker: I) -> Self {
        Self {
            learn,
            store,
            invoker,
        }
    }

    /// Runs the CLI app, which dispatches to the appropriate handler and returns an exit code.
    pub fn run(&self) -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        match dispatch(&args) {
            Action::Install(args) => self.run_install(&args),
            Action::Invoke(args) => self.run_invoke(&args),
            Action::Static(args) => self.run_static(&args),
        }
    }

    /// Runs the `install` subcommand, which parses the spec and persists the model.
    fn run_install(&self, args: &[String]) -> ExitCode {
        let matches = match build_cli_command(None).try_get_matches_from(args) {
            Ok(matches) => matches,
            Err(err) => {
                let _ = err.print();
                return ExitCode::from(err.exit_code() as u8);
            }
        };
        let install = matches
            .subcommand_matches("install")
            .expect("install subcommand is required");
        let name = install.get_one::<String>("name").expect("name is required");
        let spec_source = install
            .get_one::<String>("spec_source")
            .expect("spec_source is required");
        let base_url = install.get_one::<String>("base_url").map(String::as_str);
        match self.install(name, spec_source, base_url) {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    }

    /// Runs the static command tree, which is just for help and tests.
    fn run_static(&self, args: &[String]) -> ExitCode {
        match build_cli_command(None).try_get_matches_from(args) {
            Ok(_) => ExitCode::SUCCESS,
            Err(err) => {
                let _ = err.print();
                ExitCode::from(err.exit_code() as u8)
            }
        }
    }

    /// Runs the dynamic command tree for an installed API.
    fn run_invoke(&self, args: &[String]) -> ExitCode {
        // The first positional argument is the API name, which we use to load the model.
        let api_name = &args[1];
        let model = match self.store.load_by_name(api_name) {
            Ok(model) => model,
            Err(err) => {
                match err {
                    DomainError::NotFound { .. } => eprintln!(
                        "error: {err}; install it first with 'clining install <name> <spec>'"
                    ),
                    _ => eprintln!("error: {err}"),
                }
                return ExitCode::FAILURE;
            }
        };
        // Parse the full argv against the unified tree: the API name is a real
        // subcommand, so usage/help render the invocation exactly as typed.
        let matches = match build_cli_command(Some(&model)).try_get_matches_from(args) {
            Ok(matches) => matches,
            Err(err) => {
                let _ = err.print();
                return ExitCode::from(err.exit_code() as u8);
            }
        };
        // Extract the group and command names and their matches from the parsed clap matches.
        let api_matches = match matches.subcommand_matches(api_name.as_str()) {
            Some(matches) => matches,
            None => {
                eprintln!("error: missing command group or command");
                return ExitCode::FAILURE;
            }
        };
        let group_name = api_matches.subcommand_name().map(str::to_owned);
        let group_matches = group_name
            .as_ref()
            .and_then(|name| api_matches.subcommand_matches(name));
        let command_name = group_matches
            .and_then(|m| m.subcommand_name())
            .map(str::to_owned);
        let command_matches = command_name
            .as_ref()
            .and_then(|name| group_matches.and_then(|m| m.subcommand_matches(name)));

        let (group_name, command_name) = match (group_name, command_name) {
            (Some(group), Some(command)) => (group, command),
            _ => {
                eprintln!("error: missing command group or command");
                return ExitCode::FAILURE;
            }
        };
        // Look up the group and operation in the model to get their definitions.
        let operation_group = match model.operation_groups.iter().find(|g| g.name == group_name) {
            Some(group) => group,
            None => {
                eprintln!("error: {}", unknown_group(&group_name, &model));
                return ExitCode::FAILURE;
            }
        };
        let operation = match operation_group
            .operations
            .iter()
            .find(|c| c.name == command_name)
        {
            Some(operation) => operation,
            None => {
                eprintln!(
                    "error: {}",
                    unknown_operation(&command_name, operation_group)
                );
                return ExitCode::FAILURE;
            }
        };
        // Collect the parameter values from the command matches into a HashMap keyed by CLI name.
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        for param in operation.path_params.iter().chain(&operation.query_params) {
            if let Some(values) =
                command_matches.and_then(|m| m.get_many::<String>(&param.canonical_name))
            {
                params.insert(param.canonical_name.clone(), values.cloned().collect());
            }
        }
        // Read the request body from stdin, if any.
        let body = read_stdin_body();
        // Invoke the command using the service, passing the resolved request.
        let invocation = ApiInvocationRequest::new(model.base_url.clone(), operation, params, body);
        let service = InvokeOperationService::new(&self.invoker);
        let response = match service.invoke(&invocation) {
            Ok(response) => response,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        };
        // Write the response status and headers to stderr for visibility.
        let mut stderr = std::io::stderr().lock();
        let _ = writeln!(
            stderr,
            "HTTP/1.1 {}{}",
            response.status,
            status_text(response.status)
        );
        for (name, value) in &response.headers {
            let _ = writeln!(stderr, "{name}: {value}");
        }
        let _ = stderr.flush();
        // Write the response body to stdout byte-exact.
        if !write_response_body(&response.body) {
            return ExitCode::FAILURE;
        }
        // Return success for 2xx responses, failure otherwise.
        if (200..300).contains(&response.status) {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    /// Installs an API model by learning it from the spec and persisting it to the store.
    fn install(
        &self,
        name: &str,
        spec_source: &str,
        base_url: Option<&str>,
    ) -> Result<(), DomainError> {
        let model = self.learn.learn(name, spec_source, base_url)?;
        let groups = model.operation_groups.len();
        let commands = model
            .operation_groups
            .iter()
            .map(|g| g.operations.len())
            .sum::<usize>();
        println!(
            "Installed {} ({} commands, {} groups)",
            model.name, commands, groups
        );
        Ok(())
    }
}

/// Returns a `DomainError::Parameter` for an unknown command group, listing valid groups.
fn unknown_group(group_name: &str, model: &ApiModel) -> DomainError {
    let names: Vec<&str> = model
        .operation_groups
        .iter()
        .map(|g| g.name.as_str())
        .collect();
    DomainError::Parameter {
        message: format!(
            "unknown command group '{group_name}'; valid groups: {}",
            names.join(", ")
        ),
    }
}

/// Returns a `DomainError::Parameter` for an unknown command, listing valid commands.
fn unknown_operation(command_name: &str, group: &ApiOperationGroup) -> DomainError {
    let names: Vec<&str> = group.operations.iter().map(|c| c.name.as_str()).collect();
    DomainError::Parameter {
        message: format!(
            "unknown command '{command_name}' in group '{}'; valid commands: {}",
            group.name,
            names.join(", ")
        ),
    }
}

/// Writes response bytes to stdout byte-exact. A closed downstream pipe is
/// tolerated (standard CLI behavior), matching SIGPIPE-safe semantics.
fn write_response_body(bytes: &[u8]) -> bool {
    let mut out = std::io::stdout().lock();
    match out.write_all(bytes).and_then(|()| out.flush()) {
        Ok(()) => true,
        Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => true,
        Err(err) => {
            eprintln!("error: failed to write response body: {err}");
            false
        }
    }
}

/// Returns the request body read from stdin, or None if stdin is empty or unreadable.
fn read_stdin_body() -> Option<Vec<u8>> {
    if std::io::stdin().is_terminal() {
        return None;
    }
    let mut bytes = Vec::new();
    if std::io::stdin().read_to_end(&mut bytes).is_err() {
        return None;
    }
    if bytes.is_empty() { None } else { Some(bytes) }
}

/// Returns a static string for the HTTP status code, or an empty string if unknown.
fn status_text(status: u16) -> &'static str {
    match status {
        200 => " OK",
        201 => " Created",
        202 => " Accepted",
        204 => " No Content",
        301 => " Moved Permanently",
        302 => " Found",
        304 => " Not Modified",
        400 => " Bad Request",
        401 => " Unauthorized",
        403 => " Forbidden",
        404 => " Not Found",
        405 => " Method Not Allowed",
        409 => " Conflict",
        422 => " Unprocessable Entity",
        429 => " Too Many Requests",
        500 => " Internal Server Error",
        502 => " Bad Gateway",
        503 => " Service Unavailable",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use crate::domain::model::{
        ApiOperation, ApiOperationGroup, BodySpec, HttpMethod, ModelVersion, Param, ResponseSpec,
        SchemaSpec,
    };

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: BTreeMap::new(),
            operation_groups: vec![ApiOperationGroup {
                name: "pets".to_owned(),
                description: Some("Everything about your pets".to_owned()),
                operations: vec![ApiOperation {
                    name: "get-pet".to_owned(),
                    summary: Some("Get a pet".to_owned()),
                    method: HttpMethod::Get,
                    path: "/pets/{petId}".to_owned(),
                    path_params: vec![Param {
                        name: "petId".to_owned(),
                        canonical_name: "pet-id".to_owned(),
                        required: true,
                        description: Some("Numeric id of the pet".to_owned()),
                    }],
                    query_params: vec![Param {
                        name: "status".to_owned(),
                        canonical_name: "status".to_owned(),
                        required: false,
                        description: None,
                    }],
                    request_body: Some(BodySpec {
                        required: true,
                        content_type: "application/json".to_owned(),
                        schema: Some(SchemaSpec::Object {
                            properties: BTreeMap::new(),
                            extra_json: None,
                        }),
                    }),
                    responses: vec![],
                }],
            }],
        }
    }

    fn model_with_registry() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: BTreeMap::from([(
                "Order".to_owned(),
                SchemaSpec::Object {
                    properties: BTreeMap::from([
                        (
                            "id".to_owned(),
                            SchemaSpec::Primitive {
                                raw_json:
                                    r#"{"description":"Order id","minimum":1,"type":"integer"}"#
                                        .to_owned(),
                            },
                        ),
                        (
                            "petId".to_owned(),
                            SchemaSpec::Primitive {
                                raw_json: r#"{"type":"integer"}"#.to_owned(),
                            },
                        ),
                    ]),
                    extra_json: Some(r#"{"required":["id"]}"#.to_owned()),
                },
            )]),
            operation_groups: vec![ApiOperationGroup {
                name: "store".to_owned(),
                description: None,
                operations: vec![ApiOperation {
                    name: "place-order".to_owned(),
                    summary: None,
                    method: HttpMethod::Post,
                    path: "/store/orders".to_owned(),
                    path_params: vec![],
                    query_params: vec![],
                    request_body: Some(BodySpec {
                        required: true,
                        content_type: "application/json".to_owned(),
                        schema: Some(SchemaSpec::Ref {
                            ref_id: "Order".to_owned(),
                        }),
                    }),
                    responses: vec![ResponseSpec {
                        status_code: "200".to_owned(),
                        content_type: "application/json".to_owned(),
                        schema: Some(SchemaSpec::Ref {
                            ref_id: "Order".to_owned(),
                        }),
                    }],
                }],
            }],
        }
    }

    #[test]
    fn install_args_parse_with_optional_base_url() {
        let cmd = build_cli_command(None);
        cmd.clone().debug_assert();
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "install",
                "pets",
                "spec.json",
                "--base-url",
                "http://localhost:8080",
            ])
            .unwrap();
        let install = matches.subcommand_matches("install").unwrap();
        assert_eq!(
            install.get_one::<String>("name").map(String::as_str),
            Some("pets")
        );
        assert_eq!(
            install.get_one::<String>("spec_source").map(String::as_str),
            Some("spec.json")
        );
        assert_eq!(
            install.get_one::<String>("base_url").map(String::as_str),
            Some("http://localhost:8080")
        );
    }

    #[test]
    fn install_args_parse_without_base_url() {
        let cmd = build_cli_command(None);
        let matches = cmd
            .try_get_matches_from(["clining", "install", "pets", "spec.json"])
            .unwrap();
        let install = matches.subcommand_matches("install").unwrap();
        assert_eq!(
            install.get_one::<String>("name").map(String::as_str),
            Some("pets")
        );
        assert_eq!(
            install.get_one::<String>("spec_source").map(String::as_str),
            Some("spec.json")
        );
        assert_eq!(install.get_one::<String>("base_url"), None);
    }

    #[test]
    fn static_help_lists_install_and_api_placeholder() {
        let mut cmd = build_cli_command(None);
        cmd.clone().debug_assert();
        let help = cmd.render_help().to_string();
        assert!(help.contains("install"), "{help}");
        assert!(help.contains("<api>"), "{help}");
        assert!(help.contains("Commands for an installed API"), "{help}");
    }

    #[test]
    fn dynamic_tree_parses_group_command_and_params() {
        let cmd = build_cli_command(Some(&sample_model()));
        cmd.clone().debug_assert();
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "pets",
                "pets",
                "get-pet",
                "--pet-id",
                "42",
                "--status",
                "available",
            ])
            .unwrap();
        assert_eq!(matches.subcommand_name(), Some("pets"));
        let api = matches.subcommand_matches("pets").unwrap();
        assert_eq!(api.subcommand_name(), Some("pets"));
        let group = api.subcommand_matches("pets").unwrap();
        assert_eq!(group.subcommand_name(), Some("get-pet"));
        let command = group.subcommand_matches("get-pet").unwrap();
        assert_eq!(
            command
                .get_many::<String>("pet-id")
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["42"]
        );
        assert_eq!(
            command
                .get_many::<String>("status")
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["available"]
        );
    }

    #[test]
    fn dynamic_tree_rejects_missing_required_path_param() {
        let cmd = build_cli_command(Some(&sample_model()));
        let err = cmd
            .try_get_matches_from(["clining", "pets", "pets", "get-pet"])
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn api_help_lists_groups_with_descriptions() {
        let cmd = build_cli_command(Some(&sample_model()));
        cmd.clone().debug_assert();
        let mut api = cmd.find_subcommand("pets").unwrap().clone();
        api.clone().debug_assert();
        let help = api.render_help().to_string();
        assert!(help.contains("pets"), "{help}");
        assert!(help.contains("Everything about your pets"), "{help}");
        assert!(help.contains("Commands for API 'pets'"), "{help}");
    }

    #[test]
    fn group_help_lists_commands_with_summaries() {
        let cmd = build_cli_command(Some(&sample_model()));
        let mut group = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .clone();
        group.clone().debug_assert();
        let help = group.render_help().to_string();
        assert!(help.contains("get-pet"), "{help}");
        assert!(help.contains("Get a pet"), "{help}");
    }

    #[test]
    fn command_help_shows_params_and_body_schema() {
        let cmd = build_cli_command(Some(&sample_model()));
        let mut command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone();
        command.clone().debug_assert();
        let help = command.render_help().to_string();
        assert!(help.contains("--pet-id"), "{help}");
        assert!(help.contains("[required]"), "{help}");
        assert!(help.contains("Request: GET /pets/{petId}"), "{help}");
        assert!(help.contains("application/json"), "{help}");
        assert!(help.contains("required"), "{help}");
        assert!(help.contains("schema: {\"type\":\"object\"}"), "{help}");
    }

    #[test]
    fn param_help_text_marks_required() {
        let required = Param {
            name: "petId".to_owned(),
            canonical_name: "pet-id".to_owned(),
            required: true,
            description: Some("Numeric id".to_owned()),
        };
        assert_eq!(param_help_text(&required), "Numeric id [required]");
        let optional = Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
        };
        assert_eq!(param_help_text(&optional), "");
    }

    #[test]
    fn command_footer_includes_request_and_body() {
        let cmd = build_cli_command(Some(&sample_model()));
        let command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone();
        let footer = command.get_after_help().map(|s| s.to_string());
        let footer = footer.unwrap();
        assert!(footer.contains("Request: GET /pets/{petId}"), "{footer}");
        assert!(
            footer.contains("Body: application/json (required), schema: {\"type\":\"object\"}"),
            "{footer}"
        );
    }

    #[test]
    fn ref_body_renders_expanded_tree_without_raw_ref() {
        let cmd = build_cli_command(Some(&model_with_registry()));
        let command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("store")
            .unwrap()
            .find_subcommand("place-order")
            .unwrap()
            .clone();
        let footer = command.get_after_help().map(|s| s.to_string());
        let footer = footer.unwrap();
        assert!(footer.contains("schema: {\"properties\":"), "{footer}");
        assert!(footer.contains("\"description\":\"Order id\""), "{footer}");
        assert!(footer.contains("\"minimum\":1"), "{footer}");
        assert!(footer.contains("\"required\":[\"id\"]"), "{footer}");
        assert!(!footer.contains("\"$ref\""), "{footer}");
    }

    #[test]
    fn cyclic_ref_renders_marker_at_cycle_point() {
        let model = ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: BTreeMap::from([(
                "Node".to_owned(),
                SchemaSpec::Object {
                    properties: BTreeMap::from([(
                        "next".to_owned(),
                        SchemaSpec::Ref {
                            ref_id: "Node".to_owned(),
                        },
                    )]),
                    extra_json: None,
                },
            )]),
            operation_groups: vec![],
        };
        let rendered = render_body_schema(
            &model,
            &Some(SchemaSpec::Ref {
                ref_id: "Node".to_owned(),
            }),
        );
        let value: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(value["type"], "object");
        assert_eq!(
            value["properties"]["next"]["$ref"],
            "#/components/schemas/Node"
        );
    }

    #[test]
    fn unknown_schema_renders_wrapped_json() {
        let model = sample_model();
        let rendered = render_body_schema(
            &model,
            &Some(SchemaSpec::Unknown {
                raw_json: r#"{"x-extension":true}"#.to_owned(),
            }),
        );
        assert_eq!(rendered, r#"{"x-extension":true}"#);
    }

    #[test]
    fn dispatch_routes_by_first_positional() {
        let args: Vec<String> = vec![
            "clining".into(),
            "install".into(),
            "pets".into(),
            "spec.json".into(),
        ];
        assert!(matches!(dispatch(&args), Action::Install(_)));
        let args: Vec<String> = vec!["clining".into(), "pets".into(), "pets".into()];
        assert!(matches!(dispatch(&args), Action::Invoke(_)));
        let args: Vec<String> = vec!["clining".into(), "--help".into()];
        assert!(matches!(dispatch(&args), Action::Static(_)));
        let args: Vec<String> = vec!["clining".into()];
        assert!(matches!(dispatch(&args), Action::Static(_)));
    }

    #[test]
    fn status_text_known_and_unknown() {
        assert_eq!(status_text(200), " OK");
        assert_eq!(status_text(404), " Not Found");
        assert_eq!(status_text(599), "");
    }
}
