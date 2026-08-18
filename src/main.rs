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
    let store = JsonFileStore::new(model_store_root());
    let learn = LearnApiService::new(SourceLoader, Parser, &store);
    let invoker = match ReqwestHttpClient::new() {
        Ok(client) => client,
        Err(err) => {
            eprintln!("error: {err}");
            return ExitCode::FAILURE;
        }
    };
    CliApp::new(learn, &store, invoker).run()
}

fn model_store_root() -> PathBuf {
    if let Some(dir) = std::env::var_os("CLINING_DIR") {
        return PathBuf::from(dir);
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".clining");
    }
    PathBuf::from(".clining")
}
