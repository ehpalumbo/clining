//! clap CLI: static `install` subcommand plus dynamic per-API command tree (Phases 2-4).

use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Args, Command, Parser, Subcommand};

use crate::application::describe::{BodyHelp, CommandHelp, DescribeService, GroupHelp, ParamHelp};
use crate::application::invoke_operation::InvokeOperationService;
use crate::application::learn_api::LearnApiService;
use crate::domain::errors::DomainError;
use crate::domain::model::{ApiInvocationRequest, ApiModel, ApiOperationGroup};
use crate::domain::ports::{ApiStore, HttpInvoker, OpenApiParser, SpecLoader};

#[derive(Debug, Parser)]
#[command(
    name = "clining",
    version,
    about = "Expose OpenAPI-documented HTTP APIs as local CLI commands"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: CliCommand,
}

#[derive(Debug, Subcommand)]
pub enum CliCommand {
    /// Install an API from an OpenAPI 3.0 spec.
    Install(InstallArgs),
}

#[derive(Debug, Args)]
pub struct InstallArgs {
    /// Name under which to store the API model (~/.clining/<name>.json).
    pub name: String,

    /// Path or http(s) URL of an OpenAPI 3.0 JSON document.
    pub spec_source: String,

    /// Override the base URL taken from servers[0].url.
    #[arg(long)]
    pub base_url: Option<String>,
}

/// Builds a dynamic clap tree for an installed model: `<group>` subcommands
/// each containing `<command>` subcommands with per-parameter `--long` args.
/// Help text comes from the `DescribeService` use case so that every level of
/// `--help` renders useful, spec-derived information.
pub fn build_api_command(model: &ApiModel) -> Command {
    let help = DescribeService::describe(model);
    let mut top = Command::new("clining")
        .about(format!("Commands for API '{}'", help.name))
        .subcommand_required(true);
    for group in &help.groups {
        let mut group_cmd = Command::new(group.name.clone())
            .subcommand_required(true)
            .about(group_about(group));
        for operation in &group.commands {
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
            command_cmd = command_cmd.after_help(command_footer(operation));
            group_cmd = group_cmd.subcommand(command_cmd);
        }
        top = top.subcommand(group_cmd);
    }
    top
}

/// Help text for a group subcommand: the tag description when present, else a
/// command-count summary.
fn group_about(group: &GroupHelp) -> String {
    match &group.description {
        Some(description) => description.clone(),
        None => format!("{} commands", group.commands.len()),
    }
}

/// Help text for a parameter argument: its description plus a required marker.
fn param_help_text(param: &ParamHelp) -> String {
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
/// operation declares a body, its content type, requiredness, and schema
/// summary.
fn command_footer(command: &CommandHelp) -> String {
    let mut text = format!("Request: {} {}", command.method.as_str(), command.path);
    if let Some(body) = &command.body {
        text.push_str(&format!(
            "\nBody: {} ({})",
            body.content_type,
            body_requiredness(body)
        ));
        text.push_str(&format!(", schema: {}\n", body.schema_summary));
    }
    text
}

fn body_requiredness(body: &BodyHelp) -> &'static str {
    if body.required {
        "required"
    } else {
        "optional"
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
        let result = match Cli::try_parse_from(args) {
            Ok(cli) => match cli.command {
                CliCommand::Install(install_args) => self.install(&install_args),
            },
            Err(err) => {
                let _ = err.print();
                return ExitCode::from(err.exit_code() as u8);
            }
        };
        match result {
            Ok(()) => ExitCode::SUCCESS,
            Err(err) => {
                eprintln!("error: {err}");
                ExitCode::FAILURE
            }
        }
    }

    /// Runs the static command tree, which is just for help and tests.
    fn run_static(&self, args: &[String]) -> ExitCode {
        match Cli::try_parse_from(args) {
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
        // The second positional argument is the command group, and the third is the command.
        let mut tree_args = Vec::with_capacity(args.len().saturating_sub(1));
        tree_args.push(args[1].clone());
        tree_args.extend_from_slice(&args[2..]);
        let matches = match build_api_command(&model).try_get_matches_from(&tree_args) {
            Ok(matches) => matches,
            Err(err) => {
                let _ = err.print();
                return ExitCode::from(err.exit_code() as u8);
            }
        };
        // Extract the group and command names and their matches from the parsed clap matches.
        let group_name = matches.subcommand_name().map(str::to_owned);
        let group_matches = group_name
            .as_ref()
            .and_then(|name| matches.subcommand_matches(name));
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
    fn install(&self, args: &InstallArgs) -> Result<(), DomainError> {
        let model = self
            .learn
            .learn(&args.name, &args.spec_source, args.base_url.as_deref())?;
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

    use crate::domain::model::{
        ApiOperation, ApiOperationGroup, BodySpec, HttpMethod, ModelVersion, Param,
    };

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
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
                        schema_json: Some(r#"{"type":"object"}"#.to_owned()),
                    }),
                }],
            }],
        }
    }

    #[test]
    fn install_args_parse_with_optional_base_url() {
        let CliCommand::Install(args) = Cli::try_parse_from([
            "clining",
            "install",
            "pets",
            "spec.json",
            "--base-url",
            "http://localhost:8080",
        ])
        .unwrap()
        .command;
        assert_eq!(args.name, "pets");
        assert_eq!(args.spec_source, "spec.json");
        assert_eq!(args.base_url.as_deref(), Some("http://localhost:8080"));
    }

    #[test]
    fn install_args_parse_without_base_url() {
        let CliCommand::Install(args) =
            Cli::try_parse_from(["clining", "install", "pets", "spec.json"])
                .unwrap()
                .command;
        assert_eq!(args.base_url, None);
    }

    #[test]
    fn dynamic_tree_parses_group_command_and_params() {
        let cmd = build_api_command(&sample_model());
        cmd.clone().debug_assert();
        let matches = cmd
            .try_get_matches_from([
                "clining",
                "pets",
                "get-pet",
                "--pet-id",
                "42",
                "--status",
                "available",
            ])
            .unwrap();
        assert_eq!(matches.subcommand_name(), Some("pets"));
        let group = matches.subcommand_matches("pets").unwrap();
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
        let cmd = build_api_command(&sample_model());
        let err = cmd
            .try_get_matches_from(["clining", "pets", "get-pet"])
            .unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn api_help_lists_groups_with_descriptions() {
        let mut cmd = build_api_command(&sample_model());
        cmd.clone().debug_assert();
        let help = cmd.render_help().to_string();
        assert!(help.contains("pets"), "{help}");
        assert!(help.contains("Everything about your pets"), "{help}");
        assert!(help.contains("Commands for API 'pets'"), "{help}");
    }

    #[test]
    fn group_help_lists_commands_with_summaries() {
        let cmd = build_api_command(&sample_model());
        let mut group = cmd.find_subcommand("pets").unwrap().clone();
        group.clone().debug_assert();
        let help = group.render_help().to_string();
        assert!(help.contains("get-pet"), "{help}");
        assert!(help.contains("Get a pet"), "{help}");
    }

    #[test]
    fn command_help_shows_params_and_body_schema() {
        let cmd = build_api_command(&sample_model());
        let mut command = cmd
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
        assert!(help.contains("schema: object"), "{help}");
    }

    #[test]
    fn param_help_text_marks_required() {
        let required = ParamHelp {
            name: "petId".to_owned(),
            canonical_name: "pet-id".to_owned(),
            required: true,
            description: Some("Numeric id".to_owned()),
        };
        assert_eq!(param_help_text(&required), "Numeric id [required]");
        let optional = ParamHelp {
            name: "status".to_owned(),
            canonical_name: "status".to_owned(),
            required: false,
            description: None,
        };
        assert_eq!(param_help_text(&optional), "");
    }

    #[test]
    fn command_footer_includes_request_and_body() {
        let cmd = build_api_command(&sample_model());
        let command = cmd
            .find_subcommand("pets")
            .unwrap()
            .find_subcommand("get-pet")
            .unwrap()
            .clone();
        let footer = command.get_after_help().map(|s| s.to_string());
        let footer = footer.unwrap();
        assert!(footer.contains("Request: GET /pets/{petId}"), "{footer}");
        assert!(
            footer.contains("Body: application/json (required), schema: object"),
            "{footer}"
        );
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
