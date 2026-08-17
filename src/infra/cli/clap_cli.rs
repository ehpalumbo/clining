//! clap CLI: static `install` subcommand plus dynamic per-API command tree (Phases 2-4).

use std::process::ExitCode;

use clap::{Args, CommandFactory, Parser, Subcommand};

use crate::application::learn_api::LearnApiService;
use crate::domain::errors::DomainError;
use crate::domain::ports::{ApiStore, OpenApiParser, SpecLoader};

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

pub struct CliApp<L, P, S>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
{
    learn: LearnApiService<L, P, S>,
}

impl<L, P, S> CliApp<L, P, S>
where
    L: SpecLoader,
    P: OpenApiParser,
    S: ApiStore,
{
    pub fn new(learn: LearnApiService<L, P, S>) -> Self {
        Self { learn }
    }

    pub fn run(&self) -> ExitCode {
        let cli = Cli::parse();
        match cli.command {
            CliCommand::Install(args) => match self.install(&args) {
                Ok(()) => ExitCode::SUCCESS,
                Err(err) => {
                    eprintln!("error: {err}");
                    ExitCode::FAILURE
                }
            },
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

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
}
