mod cli;
mod codegen;
mod codex_compat;
mod error;
mod spec;

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cli::{Command, SpecCommand};
use error::{Error, Result};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let command = cli::parse(std::env::args().skip(1))?;
    if matches!(command, Command::Help) {
        print!("{}", cli::USAGE);
        return Ok(());
    }

    let repository_root = repository_root()?;
    match command {
        Command::Spec(SpecCommand::Fetch(arguments)) => spec::fetch(&repository_root, &arguments),
        Command::Spec(SpecCommand::Verify) => spec::verify(&repository_root),
        Command::Codegen { check } => {
            spec::verify(&repository_root)?;
            codegen::run(&repository_root, check)
        }
        Command::Check => {
            spec::verify(&repository_root)?;
            codegen::run(&repository_root, true)
        }
        Command::Help => Ok(()),
    }
}

fn repository_root() -> Result<PathBuf> {
    let manifest_directory = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_directory
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            Error::message(format!(
                "xtask manifest directory has no parent: {}",
                manifest_directory.display()
            ))
        })
}
