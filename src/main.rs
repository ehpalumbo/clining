#![deny(unsafe_code)]

mod application;
mod domain;
mod infra;

use std::path::PathBuf;
use std::process::ExitCode;

use application::learn_api::LearnApiService;
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
                "fatal: Could not determine model store directory. Please set the CLINING_DIR environment variable or ensure that the HOME environment variable is set."
            );
            return ExitCode::FAILURE;
        }
    };
    let store = JsonFileStore::new(store_root);
    let learn = LearnApiService::new(SourceLoader, Parser, &store);
    let invoker = match ReqwestHttpClient::new() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("fatal: Failed to initialize HTTP client: {e}");
            return ExitCode::FAILURE;
        }
    };
    CliApp::new(learn, &store, invoker).run()
}

/// Returns the path to the model store directory, or `None` if it cannot be determined.
fn model_store_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLINING_DIR") {
        return Some(PathBuf::from(dir));
    }
    if let Some(home) = std::env::var_os("HOME") {
        return Some(PathBuf::from(home).join(".clining"));
    }
    None
}
