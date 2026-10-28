#![deny(unsafe_code)]

mod application;
mod domain;
mod infra;

use std::path::PathBuf;
use std::process::ExitCode;

use application::api_invocation::ApiInvocationService;
use application::api_model::ApiModelService;
use infra::auth::LocalTokenSource;
use infra::cli::clap_cli::CliApp;
use infra::http::reqwest_http::ReqwestHttpClient;
use infra::openapi::parser::Parser;
use infra::source::loader::SourceLoader;
use infra::storage::json_file_store::JsonFileStore;

fn main() -> ExitCode {
    let store_root = match model_store_root() {
        Some(path) => path,
        None => {
            eprintln!(
                "fatal: Could not determine model store directory. Please set the CLINING_DIR environment variable or ensure that the HOME or USERPROFILE environment variable is set."
            );
            return ExitCode::FAILURE;
        }
    };
    let store = JsonFileStore::new(store_root);
    let invoker = match ReqwestHttpClient::new() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("fatal: Failed to initialize HTTP client: {e}");
            return ExitCode::FAILURE;
        }
    };

    let models = ApiModelService::new(SourceLoader, Parser, store, LocalTokenSource);
    let invoke_service = ApiInvocationService::new(invoker, LocalTokenSource);

    CliApp::new(models, invoke_service).run()
}

/// Returns the path to the model store directory, or `None` if it cannot be determined.
fn model_store_root() -> Option<PathBuf> {
    resolve_store_root(
        std::env::var_os("CLINING_DIR").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
        std::env::var_os("USERPROFILE").map(PathBuf::from),
    )
}

/// Resolves the store root given optional environment overrides and home paths.
///
/// Precedence order:
/// 1. `CLINING_DIR` (overrides everything on all platforms)
/// 2. `HOME` (standard Unix / Git Bash)
/// 3. `USERPROFILE` (standard Windows PowerShell / cmd)
fn resolve_store_root(
    clining_dir: Option<PathBuf>,
    home: Option<PathBuf>,
    userprofile: Option<PathBuf>,
) -> Option<PathBuf> {
    if let Some(dir) = clining_dir {
        return Some(dir);
    }
    if let Some(home) = home {
        return Some(home.join(".clining"));
    }
    if let Some(profile) = userprofile {
        return Some(profile.join(".clining"));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clining_dir_precedence_over_home_and_userprofile() {
        let root = resolve_store_root(
            Some(PathBuf::from("/custom/dir")),
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from(r"C:\Users\user")),
        );
        assert_eq!(root, Some(PathBuf::from("/custom/dir")));
    }

    #[test]
    fn home_precedence_over_userprofile() {
        let root = resolve_store_root(
            None,
            Some(PathBuf::from("/home/user")),
            Some(PathBuf::from(r"C:\Users\user")),
        );
        assert_eq!(root, Some(PathBuf::from("/home/user/.clining")));
    }

    #[test]
    fn userprofile_fallback_when_home_unset() {
        let root = resolve_store_root(None, None, Some(PathBuf::from(r"C:\Users\user")));
        assert_eq!(root, Some(PathBuf::from(r"C:\Users\user").join(".clining")));
    }

    #[test]
    fn store_root_returns_none_when_all_unset() {
        let root = resolve_store_root(None, None, None);
        assert_eq!(root, None);
    }
}
