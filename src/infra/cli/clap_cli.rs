//! clap CLI: a single builder-API command tree — a static `api` management
//! group (currently holding `install`) plus a per-API subcommand tree added
//! once the installed model is loaded.

use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Command};

use crate::application::api_invocation::ApiInvocationService;
use crate::application::api_model::ApiModelService;
use crate::application::auth::auth_from_options;
use crate::domain::errors::DomainError;
use crate::domain::headers::CustomHeader;
use crate::domain::model::{ApiInvocationRequest, ApiModel, ApiOperation, ApiOperationGroup};
use crate::infra::cli::help::{command_after_help, param_help_text};

/// Internal clap argument ID for custom headers. Kebab-casing never emits
/// `_`, so this cannot collide with a generated parameter ID.
const CUSTOM_HEADER_ARG_ID: &str = "clining_header";
/// Long name for the custom-header flag when the operation does not claim it.
const CUSTOM_HEADER_LONG: &str = "clining-header";
const CUSTOM_HEADER_HELP: &str = "Custom header as 'Name: value' (repeatable; sends separate lines and overrides generated headers).";

/// Builds the full CLI command tree: the static `api` management group plus,
/// when a model is provided, the installed API as a nested subcommand tree.
/// Without a model, a placeholder `<api>` entry keeps the invocation shape
/// visible in `clining --help`.
pub fn build_cli_command(model: Option<&ApiModel>) -> Command {
    let top = Command::new("clining")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Expose OpenAPI-documented HTTP APIs as local CLI commands")
        .disable_help_subcommand(true)
        .subcommand_required(true)
        .subcommand(api_group());
    match model {
        Some(model) => top.subcommand(api_command(model)),
        None => top.subcommand(api_placeholder_command()),
    }
}

/// Management verbs of the top-level `api` group. The single table feeds both
/// the static clap tree (help) and the dispatch membership check, so help and
/// routing cannot drift. Future management commands (`list`, `edit`, `remove`)
/// become table entries.
const MANAGEMENT_VERBS: &[&str] = &["install"];

/// Returns whether `verb` is a management verb of the `api` group.
fn management_verb(verb: Option<&str>) -> bool {
    verb.is_some_and(|verb| MANAGEMENT_VERBS.contains(&verb))
}

/// Builds the `verb` command for the `api` group from the verb table.
fn management_verb_command(verb: &str) -> Command {
    match verb {
        "install" => install_command(),
        _ => unreachable!("unknown management verb: {verb}"),
    }
}

/// Builds the static `api` management group containing the verb table's
/// subcommands.
fn api_group() -> Command {
    let mut group = Command::new("api")
        .about("Manage installed API models")
        .disable_help_subcommand(true)
        .subcommand_required(true);
    for verb in MANAGEMENT_VERBS {
        group = group.subcommand(management_verb_command(verb));
    }
    group
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
        .arg(
            Arg::new("token_env")
                .long("token-env")
                .value_name("VAR")
                .conflicts_with("token_file")
                .help(
                    "Read Bearer token from environment variable VAR (stored as reference only).",
                ),
        )
        .arg(
            Arg::new("token_file")
                .long("token-file")
                .value_name("PATH")
                .conflicts_with("token_env")
                .help(
                    "Read Bearer token from file PATH (stored as reference only; relative paths are stored as absolute at install time).",
                ),
        )
}

/// Resolves a `--token-file` value to the path persisted in the model.
///
/// Absolute paths pass through verbatim; relative paths are joined onto the
/// current working directory so the stored `AuthConfig::File` reference stays
/// valid when later invocations run from a different directory. Joining (not
/// `fs::canonicalize`) preserves the user's path text and needs no existence
/// proof beyond the fail-fast resolve at install. An empty value is a
/// `Parameter` error (checked here so `--token-file ""` cannot collapse to
/// the working directory itself via `Path::join`).
fn resolve_token_file_path(path: &str) -> Result<String, DomainError> {
    if path.is_empty() {
        return Err(DomainError::Parameter {
            message: "token file path must not be empty".to_owned(),
        });
    }
    let candidate = std::path::Path::new(path);
    if candidate.is_absolute() {
        return Ok(path.to_owned());
    }
    let cwd = std::env::current_dir().map_err(|e| DomainError::Io {
        message: format!("failed to resolve current directory: {e}"),
    })?;
    Ok(cwd.join(candidate).to_string_lossy().into_owned())
}

/// Stand-in for the dynamic per-API subcommand, shown when no model is loaded.
/// The static path only parses the `api` group, flags, or no arguments, so
/// the placeholder is never actually matched.
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
            if let Some(long_about) = operation_long_about(operation) {
                command_cmd = command_cmd.long_about(long_about);
            }
            for param in &operation.path_params {
                command_cmd = command_cmd.arg(
                    Arg::new(param.canonical_name.clone())
                        .long(param.canonical_name.clone())
                        .value_name(param.name.clone())
                        .required(true)
                        .help(param_help_text(model, param)),
                );
            }
            for param in &operation.query_params {
                let mut arg = Arg::new(param.canonical_name.clone())
                    .long(param.canonical_name.clone())
                    .value_name(param.name.clone())
                    .action(ArgAction::Append)
                    .num_args(1..)
                    .help(param_help_text(model, param));
                if param.required {
                    arg = arg.required(true);
                }
                command_cmd = command_cmd.arg(arg);
            }
            let mut custom_header_arg = Arg::new(CUSTOM_HEADER_ARG_ID)
                .short('H')
                .value_name("HEADER")
                .action(ArgAction::Append)
                .num_args(1)
                .help(CUSTOM_HEADER_HELP);
            // If an API parameter owns the long name, keep it for the API and
            // expose the custom-header flag as `-H` only.
            if !operation_claims_clining_header(operation) {
                custom_header_arg = custom_header_arg.long(CUSTOM_HEADER_LONG);
            }
            command_cmd = command_cmd.arg(custom_header_arg);
            command_cmd = command_cmd.after_long_help(command_after_help(model, operation));
            group_cmd = group_cmd.subcommand(command_cmd);
        }
        api = api.subcommand(group_cmd);
    }
    api
}

/// Returns whether a path or query parameter already owns the long name used
/// by the custom-header flag.
fn operation_claims_clining_header(operation: &ApiOperation) -> bool {
    operation
        .path_params
        .iter()
        .chain(&operation.query_params)
        .any(|param| param.canonical_name == CUSTOM_HEADER_LONG)
}

/// Long help for an operation command: the summary followed by the operation
/// description when the spec provides one. The summary alone stays on `about`
/// so `-h` and group listings keep their compact form.
fn operation_long_about(operation: &ApiOperation) -> Option<String> {
    let description = operation.description.as_deref()?.trim();
    if description.is_empty() {
        return None;
    }
    match &operation.summary {
        Some(summary) => Some(format!("{summary}\n\n{description}")),
        None => Some(description.to_owned()),
    }
}

/// Help text for a group subcommand: the tag description when present, else a
/// command-count summary.
fn group_about(group: &ApiOperationGroup) -> String {
    match &group.description {
        Some(description) => description.clone(),
        None => format!("{} commands", group.operations.len()),
    }
}

/// Top-level routing decision: which parse tree the arguments belong to.
/// The variants say which parse tree handles `argv` — not what executes —
/// because help rendering can happen through clap on any route (`clining
/// pets --help` already renders help via `Invocation`).
enum Route {
    /// Static `api` management group parse tree.
    Management(Vec<String>),
    /// Runtime-built dynamic per-API parse tree.
    Invocation(Vec<String>),
    /// Top-level parse tree: help only, plus clap usage errors.
    Static(Vec<String>),
}

/// Routes the CLI arguments by the first positional argument (exact string
/// compares, mirroring the dispatcher):
///
/// - `api` followed by a management verb → `Management`;
/// - `api` otherwise (`--help`, unknown verbs, bare) → `Static` (group help
///   or a clap usage error);
/// - any other non-flag first positional → `Invocation` (this also covers the
///   root word `install`: there is no top-level install, so it fails as a
///   not-found model load whose hint points at `clining api install`);
/// - flags or no arguments at all → `Static`.
fn dispatch(argv: &[String]) -> Route {
    match argv.get(1).map(String::as_str) {
        Some("api") if management_verb(argv.get(2).map(String::as_str)) => {
            Route::Management(argv.to_vec())
        }
        Some("api") => Route::Static(argv.to_vec()),
        Some(first) if !first.starts_with('-') => Route::Invocation(argv.to_vec()),
        _ => Route::Static(argv.to_vec()),
    }
}

/// Main CLI application, holding the services needed to run commands.
pub struct CliApp {
    models: ApiModelService,
    invoker: ApiInvocationService,
}

impl CliApp {
    pub fn new(models: ApiModelService, invoker: ApiInvocationService) -> Self {
        Self { models, invoker }
    }

    /// Runs the CLI app, which dispatches to the appropriate handler and returns an exit code.
    pub fn run(&self) -> ExitCode {
        let args: Vec<String> = std::env::args().collect();
        match dispatch(&args) {
            Route::Management(args) => self.run_install(&args),
            Route::Invocation(args) => self.run_invoke(&args),
            Route::Static(args) => self.run_static(&args),
        }
    }

    /// Runs the `api install` subcommand, which parses the spec and persists
    /// the model.
    fn run_install(&self, args: &[String]) -> ExitCode {
        let matches = match build_cli_command(None).try_get_matches_from(args) {
            Ok(matches) => matches,
            Err(err) => {
                let _ = err.print();
                return ExitCode::from(err.exit_code() as u8);
            }
        };
        let install = matches
            .subcommand_matches("api")
            .and_then(|api| api.subcommand_matches("install"))
            .expect("api install subcommand is required");
        let name = install.get_one::<String>("name").expect("name is required");
        let spec_source = install
            .get_one::<String>("spec_source")
            .expect("spec_source is required");
        let base_url = install.get_one::<String>("base_url").map(String::as_str);
        let token_env = install.get_one::<String>("token_env").map(String::as_str);
        let token_file = install.get_one::<String>("token_file").map(String::as_str);
        let normalized_token_file: Option<String> = match token_file {
            Some(path) => match resolve_token_file_path(path) {
                Ok(absolute) => Some(absolute),
                Err(err) => {
                    eprintln!("error: {err}");
                    return ExitCode::FAILURE;
                }
            },
            None => None,
        };
        let auth = match auth_from_options(token_env, normalized_token_file.as_deref()) {
            Ok(auth) => auth,
            Err(err) => {
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        };
        match self.install(name, spec_source, base_url, auth) {
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
        let model = match self.models.load(api_name) {
            Ok(model) => model,
            Err(err) => {
                match err {
                    DomainError::NotFound { .. } => eprintln!(
                        "error: {err}; install it first with 'clining api install <name> <spec>'"
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
        // Parse and validate custom headers before reading stdin so malformed
        // input fails fast without consuming the body or attempting a request.
        let headers = match command_matches.and_then(|m| m.get_many::<String>(CUSTOM_HEADER_ARG_ID))
        {
            Some(values) => {
                let mut headers = Vec::new();
                for raw in values {
                    match CustomHeader::parse(raw) {
                        Ok(header) => headers.push(header),
                        Err(err) => {
                            eprintln!("error: {err}");
                            return ExitCode::FAILURE;
                        }
                    }
                }
                headers
            }
            None => Vec::new(),
        };
        // Read the request body from stdin, if any.
        let body = read_stdin_body();
        // Invoke the command using the service, passing the resolved request.
        // The stored auth reference (if any) is cloned so the service can
        // resolve the token and attach the Bearer header.
        let invocation = ApiInvocationRequest::new(
            model.base_url.clone(),
            operation,
            params,
            body,
            model.auth.clone(),
            headers,
        );
        let response = match self.invoker.invoke(&invocation) {
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
    /// `auth = None` clears any previous auth (re-install overwrite semantics).
    fn install(
        &self,
        name: &str,
        spec_source: &str,
        base_url: Option<&str>,
        auth: Option<crate::domain::model::AuthConfig>,
    ) -> Result<(), DomainError> {
        let model = self.models.learn(name, spec_source, base_url, auth)?;
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

    fn command_help(model: &ApiModel, group: &str, name: &str) -> String {
        build_cli_command(Some(model))
            .find_subcommand(group)
            .unwrap()
            .find_subcommand(group)
            .unwrap()
            .find_subcommand(name)
            .unwrap()
            .clone()
            .render_long_help()
            .to_string()
    }

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            schema_registry: BTreeMap::new(),
            auth: None,
            operation_groups: vec![ApiOperationGroup {
                name: "pets".to_owned(),
                description: Some("Everything about your pets".to_owned()),
                operations: vec![ApiOperation {
                    name: "get-pet".to_owned(),
                    summary: Some("Get a pet".to_owned()),
                    description: Some("Returns the requested pet by id.".to_owned()),
                    method: HttpMethod::Get,
                    path: "/pets/{petId}".to_owned(),
                    path_params: vec![Param {
                        name: "petId".to_owned(),
                        canonical_name: "pet-id".to_owned(),
                        required: true,
                        description: Some("Numeric id of the pet".to_owned()),
                        schema: None,
                    }],
                    query_params: vec![Param {
                        name: "status".to_owned(),
                        canonical_name: "status".to_owned(),
                        required: false,
                        description: None,
                        schema: None,
                    }],
                    request_body: Some(BodySpec {
                        required: true,
                        content_type: "application/json".to_owned(),
                        description: None,
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
            auth: None,
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
                    description: None,
                    method: HttpMethod::Post,
                    path: "/store/orders".to_owned(),
                    path_params: vec![],
                    query_params: vec![],
                    request_body: Some(BodySpec {
                        required: true,
                        content_type: "application/json".to_owned(),
                        description: None,
                        schema: Some(SchemaSpec::Ref {
                            ref_id: "Order".to_owned(),
                        }),
                    }),
                    responses: vec![ResponseSpec {
                        status_code: "200".to_owned(),
                        description: None,
                        content_type: Some("application/json".to_owned()),
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
                "api",
                "install",
                "pets",
                "spec.json",
                "--base-url",
                "http://localhost:8080",
            ])
            .unwrap();
        let install = matches
            .subcommand_matches("api")
            .unwrap()
            .subcommand_matches("install")
            .unwrap();
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
            .try_get_matches_from(["clining", "api", "install", "pets", "spec.json"])
            .unwrap();
        let install = matches
            .subcommand_matches("api")
            .unwrap()
            .subcommand_matches("install")
            .unwrap();
        assert_eq!(
            install.get_one::<String>("name").map(String::as_str),
            Some("pets")
        );
        assert_eq!(
            install.get_one::<String>("spec_source").map(String::as_str),
            Some("spec.json")
        );
        assert_eq!(install.get_one::<String>("base_url"), None);
        assert_eq!(install.get_one::<String>("token_env"), None);
        assert_eq!(install.get_one::<String>("token_file"), None);
    }

    #[test]
    fn install_args_parse_with_token_env() {
        let cmd = build_cli_command(None);
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "api",
                "install",
                "pets",
                "spec.json",
                "--token-env",
                "MY_TOKEN",
            ])
            .unwrap();
        let install = matches
            .subcommand_matches("api")
            .unwrap()
            .subcommand_matches("install")
            .unwrap();
        assert_eq!(
            install.get_one::<String>("token_env").map(String::as_str),
            Some("MY_TOKEN")
        );
        assert_eq!(install.get_one::<String>("token_file"), None);
        let auth = auth_from_options(Some("MY_TOKEN"), None).expect("valid auth options");
        assert_eq!(
            auth,
            Some(crate::domain::model::AuthConfig::Env {
                var: "MY_TOKEN".to_owned()
            })
        );
    }

    #[test]
    fn install_args_parse_with_token_file() {
        let cmd = build_cli_command(None);
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "api",
                "install",
                "pets",
                "spec.json",
                "--token-file",
                "/tmp/token.txt",
            ])
            .unwrap();
        let install = matches
            .subcommand_matches("api")
            .unwrap()
            .subcommand_matches("install")
            .unwrap();
        assert_eq!(
            install.get_one::<String>("token_file").map(String::as_str),
            Some("/tmp/token.txt")
        );
        assert_eq!(install.get_one::<String>("token_env"), None);
    }

    #[test]
    fn install_rejects_both_token_sources() {
        let cmd = build_cli_command(None);
        let err = cmd
            .try_get_matches_from([
                "clining",
                "api",
                "install",
                "pets",
                "spec.json",
                "--token-env",
                "A",
                "--token-file",
                "/tmp/t",
            ])
            .unwrap_err();
        // clap usage error (exit code 2) via conflicts_with.
        assert_eq!(err.exit_code(), 2);
        let err = auth_from_options(Some("A"), Some("/tmp/t")).unwrap_err();
        assert!(matches!(
            err,
            crate::domain::errors::DomainError::Parameter { .. }
        ));
    }

    #[test]
    fn install_help_documents_token_flags() {
        let mut cmd = build_cli_command(None);
        let install = cmd
            .find_subcommand_mut("api")
            .expect("api group")
            .find_subcommand_mut("install")
            .expect("install subcommand");
        let help = install.render_help().to_string();
        assert!(help.contains("--token-env"), "{help}");
        assert!(help.contains("--token-file"), "{help}");
        assert!(help.contains("stored as absolute"), "{help}");
    }

    #[test]
    fn token_file_absolute_path_passes_through() {
        let temp = std::env::temp_dir().join("token.txt");
        let temp_str = temp.to_str().expect("valid utf-8 temp path");
        assert_eq!(resolve_token_file_path(temp_str).unwrap(), temp_str);
    }

    #[test]
    fn token_file_relative_path_joins_current_dir() {
        let cwd = std::env::current_dir().unwrap();
        let joined = cwd.join("rel/tok.txt").to_string_lossy().into_owned();
        assert_eq!(resolve_token_file_path("rel/tok.txt").unwrap(), joined);
    }

    #[test]
    fn token_file_empty_path_is_parameter_error() {
        // Empty must not collapse to the cwd via `Path::join` (which would
        // later fail confusingly as "Is a directory"); it is a usage error.
        let err = resolve_token_file_path("").unwrap_err();
        assert!(
            matches!(err, crate::domain::errors::DomainError::Parameter { .. }),
            "{err}"
        );
        assert!(err.to_string().contains("must not be empty"), "{err}");
    }

    #[test]
    fn static_help_lists_api_group_with_install_and_placeholder() {
        let mut cmd = build_cli_command(None);
        cmd.clone().debug_assert();
        let top_help = cmd.render_help().to_string();
        assert!(top_help.contains("api"), "{top_help}");
        assert!(
            top_help.contains("Manage installed API models"),
            "{top_help}"
        );
        assert!(top_help.contains("<api>"), "{top_help}");
        assert!(
            top_help.contains("Commands for an installed API"),
            "{top_help}"
        );

        let api_help = cmd
            .find_subcommand_mut("api")
            .expect("api group")
            .find_subcommand_mut("install")
            .expect("install beneath api")
            .render_help()
            .to_string();
        assert!(api_help.contains("Install an API"), "{api_help}");
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
        let help = command_help(&sample_model(), "pets", "get-pet");
        assert!(help.contains("--pet-id"), "{help}");
        assert!(help.contains("(required)"), "{help}");
        assert!(help.contains("Request: GET /pets/{petId}"), "{help}");
        assert!(help.contains("Request body"), "{help}");
        assert!(help.contains("application/json"), "{help}");
        assert!(
            help.contains("application/json (required)\n    object"),
            "{help}"
        );
        assert!(help.contains("Responses"), "{help}");
    }

    #[test]
    fn command_long_help_places_description_after_summary() {
        let help = command_help(&sample_model(), "pets", "get-pet");
        let summary = help.find("Get a pet").expect("summary is rendered");
        let description = help
            .find("Returns the requested pet by id.")
            .expect("description is rendered");
        assert!(summary < description, "{help}");
    }

    #[test]
    fn operation_long_about_combines_summary_and_description() {
        let mut operation = sample_model().operation_groups[0].operations[0].clone();
        assert_eq!(
            operation_long_about(&operation).as_deref(),
            Some("Get a pet\n\nReturns the requested pet by id.")
        );

        operation.summary = None;
        assert_eq!(
            operation_long_about(&operation).as_deref(),
            Some("Returns the requested pet by id.")
        );

        operation.description = None;
        assert_eq!(operation_long_about(&operation), None);
    }

    #[test]
    fn group_help_keeps_summary_only() {
        let cmd = build_cli_command(Some(&sample_model()));
        let mut group = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .clone();
        let help = group.render_help().to_string();
        assert!(help.contains("Get a pet"), "{help}");
        assert!(!help.contains("Returns the requested pet by id."), "{help}");
    }

    #[test]
    fn param_help_text_marks_required() {
        let model = sample_model();
        let required = Param {
            name: "petId".to_owned(),
            canonical_name: "pet-id".to_owned(),
            required: true,
            description: Some("Numeric id".to_owned()),
            schema: None,
        };
        let optional = Param {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
            schema: None,
        };
        assert_eq!(param_help_text(&model, &required), "Numeric id (required)");
        assert_eq!(param_help_text(&model, &optional), "(optional)");
    }

    #[test]
    fn command_help_indents_structured_param_walkthrough() {
        let mut model = sample_model();
        model.schema_registry.insert(
            "Filter".to_owned(),
            SchemaSpec::Object {
                properties: BTreeMap::from([(
                    "kind".to_owned(),
                    SchemaSpec::Primitive {
                        raw_json: r#"{"enum":["a","b"],"type":"string"}"#.to_owned(),
                    },
                )]),
                extra_json: Some(r#"{"required":["kind"]}"#.to_owned()),
            },
        );
        model.operation_groups[0].operations[0].query_params[0].schema = Some(SchemaSpec::Ref {
            ref_id: "Filter".to_owned(),
        });

        let cmd = build_cli_command(Some(&model));
        let mut command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone();
        let help = command.render_help().to_string();
        assert!(help.contains("type: Filter (object)"), "{help}");
        assert!(
            help.contains(r#"kind: string (required) (enum: "a", "b")"#),
            "{help}"
        );
    }

    #[test]
    fn command_footer_includes_request_and_body() {
        let cmd = build_cli_command(Some(&sample_model()));
        let mut command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone();
        let footer = command.get_after_long_help().map(|s| s.to_string());
        let footer = footer.unwrap();
        assert!(footer.contains("Request: GET /pets/{petId}"), "{footer}");
        assert!(
            footer.contains("Request body:\n  application/json (required)\n    object"),
            "{footer}"
        );

        assert!(command.get_after_help().is_none());
        let short_help = command.render_help().to_string();
        assert!(
            !short_help.contains("Request: GET /pets/{petId}"),
            "{short_help}"
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
        let footer = command.get_after_long_help().map(|s| s.to_string());
        let footer = footer.unwrap();
        assert!(footer.contains("Order (object) {"), "{footer}");
        assert!(
            footer.contains("id: integer (required) (minimum: 1) — Order id"),
            "{footer}"
        );
        assert!(footer.contains("Responses:"), "{footer}");
        assert!(footer.contains("200 application/json"), "{footer}");
        assert!(!footer.contains("$ref"), "{footer}");
    }

    #[test]
    fn dispatch_routes_by_parse_tree() {
        let args: Vec<String> = vec![
            "clining".into(),
            "api".into(),
            "install".into(),
            "pets".into(),
            "spec.json".into(),
        ];
        assert!(matches!(dispatch(&args), Route::Management(_)));
        for statics in [
            vec!["clining", "api", "--help"],
            vec!["clining", "api"],
            vec!["clining", "api", "frobnicate"],
            vec!["clining", "--help"],
            vec!["clining"],
        ] {
            let args: Vec<String> = statics.iter().map(|s| String::from(*s)).collect();
            assert!(matches!(dispatch(&args), Route::Static(_)), "{statics:?}");
        }
        // The root word `install` is an ordinary (absent) API name now: it
        // routes to the invocation tree and fails as not-found with the
        // `clining api install` hint.
        let args: Vec<String> = vec![
            "clining".into(),
            "install".into(),
            "pets".into(),
            "spec.json".into(),
        ];
        assert!(matches!(dispatch(&args), Route::Invocation(_)));
        let args: Vec<String> = vec!["clining".into(), "pets".into(), "pets".into()];
        assert!(matches!(dispatch(&args), Route::Invocation(_)));
    }

    #[test]
    fn status_text_known_and_unknown() {
        assert_eq!(status_text(200), " OK");
        assert_eq!(status_text(404), " Not Found");
        assert_eq!(status_text(599), "");
    }

    fn get_pet_command(model: &ApiModel) -> Command {
        build_cli_command(Some(model))
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone()
    }

    #[test]
    fn custom_header_flag_is_registered_once_and_collects_in_order() {
        let model = sample_model();
        let help_cmd = get_pet_command(&model);
        help_cmd.clone().debug_assert();
        let help = help_cmd.clone().render_long_help().to_string();
        assert!(help.contains("-H, --clining-header <HEADER>"), "{help}");

        let cmd = build_cli_command(Some(&model));
        cmd.clone().debug_assert();
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "pets",
                "pets",
                "get-pet",
                "--pet-id",
                "42",
                "-H",
                "A: 1",
                "--clining-header",
                "B: 2",
            ])
            .unwrap();
        let command = matches
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("get-pet")
            .unwrap();
        assert_eq!(
            command
                .get_many::<String>(CUSTOM_HEADER_ARG_ID)
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["A: 1", "B: 2"]
        );
    }

    #[test]
    fn api_parameter_named_header_keeps_long_name_and_custom_flag_still_works() {
        let mut model = sample_model();
        model.operation_groups[0].operations[0].query_params[0].canonical_name =
            "header".to_owned();
        model.operation_groups[0].operations[0].query_params[0].name = "header".to_owned();

        let cmd = get_pet_command(&model);
        cmd.clone().debug_assert();
        let help = cmd.clone().render_long_help().to_string();
        assert!(help.contains("--header"), "{help}");
        assert!(help.contains("-H, --clining-header <HEADER>"), "{help}");

        let full = build_cli_command(Some(&model));
        full.clone().debug_assert();
        let matches = full
            .try_get_matches_from([
                "clining",
                "pets",
                "pets",
                "get-pet",
                "--pet-id",
                "42",
                "--header",
                "api-value",
                "-H",
                "X-Custom: yes",
            ])
            .unwrap();
        let command = matches
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("get-pet")
            .unwrap();
        assert_eq!(
            command
                .get_many::<String>("header")
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["api-value"]
        );
        assert_eq!(
            command
                .get_many::<String>(CUSTOM_HEADER_ARG_ID)
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["X-Custom: yes"]
        );
    }

    #[test]
    fn api_parameter_named_clining_header_keeps_long_name_and_h_remains_short_only() {
        let mut model = sample_model();
        model.operation_groups[0].operations[0].query_params[0].canonical_name =
            CUSTOM_HEADER_LONG.to_owned();
        model.operation_groups[0].operations[0].query_params[0].name =
            CUSTOM_HEADER_LONG.to_owned();

        let cmd = get_pet_command(&model);
        cmd.clone().debug_assert();
        let help = cmd.clone().render_long_help().to_string();
        assert!(help.contains("--clining-header"), "{help}");
        assert!(help.contains("-H <HEADER>"), "{help}");
        assert!(!help.contains("-H, --clining-header"), "{help}");

        let full = build_cli_command(Some(&model));
        full.clone().debug_assert();
        let matches = full
            .try_get_matches_from([
                "clining",
                "pets",
                "pets",
                "get-pet",
                "--pet-id",
                "42",
                "--clining-header",
                "api-value",
                "-H",
                "X-Custom: yes",
            ])
            .unwrap();
        let command = matches
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("pets")
            .unwrap()
            .subcommand_matches("get-pet")
            .unwrap();
        assert_eq!(
            command
                .get_many::<String>(CUSTOM_HEADER_LONG)
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["api-value"]
        );
        assert_eq!(
            command
                .get_many::<String>(CUSTOM_HEADER_ARG_ID)
                .unwrap()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["X-Custom: yes"]
        );
    }
}
