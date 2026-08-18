//! clap CLI: static `install` subcommand plus dynamic per-API command tree (Phases 2-4).

use std::collections::HashMap;
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

use clap::{Arg, ArgAction, Args, Command, CommandFactory, Parser, Subcommand};

use crate::application::invoke_command::InvokeCommandService;
use crate::application::learn_api::LearnApiService;
use crate::domain::errors::DomainError;
use crate::domain::model::ApiModel;
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

/// Static command tree for top-level help and tests.
#[allow(dead_code)]
pub fn build_static_command() -> clap::Command {
    Cli::command()
}

/// Builds a dynamic clap tree for an installed model: `<group>` subcommands
/// each containing `<command>` subcommands with per-parameter `--long` args.
pub fn build_api_command(model: &ApiModel) -> Command {
    let mut top = Command::new("clining")
        .about("Invoke a command from an installed API")
        .subcommand_required(true);
    for group in &model.command_groups {
        let mut group_cmd = Command::new(group.name.clone()).subcommand_required(true);
        for command in &group.commands {
            let mut command_cmd = Command::new(command.name.clone());
            if let Some(summary) = &command.summary {
                command_cmd = command_cmd.about(summary);
            }
            for param in &command.path_params {
                command_cmd = command_cmd.arg(
                    Arg::new(param.cli_name.clone())
                        .long(param.cli_name.clone())
                        .value_name(param.name.clone())
                        .required(true),
                );
            }
            for param in &command.query_params {
                let mut arg = Arg::new(param.cli_name.clone())
                    .long(param.cli_name.clone())
                    .value_name(param.name.clone())
                    .action(ArgAction::Append)
                    .num_args(1..);
                if param.required {
                    arg = arg.required(true);
                }
                command_cmd = command_cmd.arg(arg);
            }
            group_cmd = group_cmd.subcommand(command_cmd);
        }
        top = top.subcommand(group_cmd);
    }
    top
}

/// Top-level dispatch decision based on the first positional argument.
enum Action {
    Install(Vec<String>),
    Invoke(Vec<String>),
    Static(Vec<String>),
}

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
                eprintln!("error: {err}");
                return ExitCode::FAILURE;
            }
        };
        // The second positional argument is the command group, and the third is the command.
        let mut tree_args = Vec::with_capacity(args.len().saturating_sub(1));
        tree_args.push(args[0].clone());
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
        // Look up the group and command in the model to get their definitions.
        let group = match model.command_groups.iter().find(|g| g.name == group_name) {
            Some(group) => group,
            None => {
                eprintln!("error: unknown command group '{group_name}'");
                return ExitCode::FAILURE;
            }
        };
        let command = match group.commands.iter().find(|c| c.name == command_name) {
            Some(command) => command,
            None => {
                eprintln!("error: unknown command '{command_name}' in group '{group_name}'");
                return ExitCode::FAILURE;
            }
        };
        // Collect the parameter values from the command matches into a HashMap keyed by CLI name.
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        for param in command.path_params.iter().chain(&command.query_params) {
            if let Some(values) =
                command_matches.and_then(|m| m.get_many::<String>(&param.cli_name))
            {
                params.insert(param.cli_name.clone(), values.cloned().collect());
            }
        }
        // Read the request body from stdin, if any.
        let body = read_stdin_body();
        // Invoke the command using the service, passing all collected information
        eprintln!(
            "Invoking {group_name}/{command_name} with params: {params:?}, body length: {}",
            body.as_ref().map_or(0, Vec::len)
        );
        let service = InvokeCommandService::new(&self.store, &self.invoker);
        let response = match service.invoke(
            api_name,
            &group_name,
            &command_name,
            &params,
            body.as_deref(),
        ) {
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
        // Write the response body to stdout.
        if let Err(err) = std::io::stdout().write_all(&response.body) {
            eprintln!("error: failed to write response body: {err}");
            return ExitCode::FAILURE;
        }
        if let Err(err) = std::io::stdout().flush() {
            eprintln!("error: failed to flush stdout: {err}");
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
        let groups = model.command_groups.len();
        let commands = model
            .command_groups
            .iter()
            .map(|g| g.commands.len())
            .sum::<usize>();
        println!(
            "Installed {} ({} commands, {} groups)",
            model.name, commands, groups
        );
        Ok(())
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

    use crate::domain::model::Command as ModelCommand;
    use crate::domain::model::{CommandGroup, HttpMethod, ModelVersion, Param};

    fn sample_model() -> ApiModel {
        ApiModel {
            name: "pets".to_owned(),
            base_url: "https://example.com".to_owned(),
            version: ModelVersion::V1,
            command_groups: vec![CommandGroup {
                name: "pets".to_owned(),
                commands: vec![ModelCommand {
                    name: "get-pet".to_owned(),
                    summary: Some("Get a pet".to_owned()),
                    method: HttpMethod::Get,
                    path: "/pets/{petId}".to_owned(),
                    path_params: vec![Param {
                        name: "petId".to_owned(),
                        cli_name: "pet-id".to_owned(),
                        required: true,
                    }],
                    query_params: vec![Param {
                        name: "status".to_owned(),
                        cli_name: "status".to_owned(),
                        required: false,
                    }],
                    request_body: None,
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
    fn static_command_renders_help() {
        let mut cmd = build_static_command();
        cmd.clone().debug_assert();
        let help = cmd.render_help().to_string();
        assert!(help.contains("install"));
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
