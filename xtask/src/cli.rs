use crate::error::{Error, Result};

pub const USAGE: &str = "\
Repository maintenance commands for openai-rs

Usage:
  cargo run -p xtask -- spec fetch --rev <40-char-sha> [--url <official-raw-url>]
  cargo run -p xtask -- spec verify
  cargo run -p xtask -- drift [--from <spec-path>] --to <spec-path>
  cargo run -p xtask -- codegen [--check]
  cargo run -p xtask -- check

Commands:
  spec fetch   Fetch the audited OpenAPI snapshot from an immutable official GitHub URL.
  spec verify  Verify the committed OpenAPI bytes, identity, versions, and inventory.
  drift        Compare two OpenAPI snapshots and classify breaking, lifecycle, and additive drift.
  codegen      Run the extensible code-generation pipeline skeleton.
  check        Run spec verify followed by codegen --check.
";

#[derive(Debug, Eq, PartialEq)]
pub enum Command {
    Spec(SpecCommand),
    Drift(DriftArguments),
    Codegen { check: bool },
    Check,
    Help,
}

#[derive(Debug, Eq, PartialEq)]
pub struct DriftArguments {
    pub from: Option<String>,
    pub to: String,
}

#[derive(Debug, Eq, PartialEq)]
pub enum SpecCommand {
    Fetch(FetchArguments),
    Verify,
}

#[derive(Debug, Eq, PartialEq)]
pub struct FetchArguments {
    pub revision: String,
    pub url: Option<String>,
}

pub fn parse(arguments: impl IntoIterator<Item = String>) -> Result<Command> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    let Some(command) = arguments.first().map(String::as_str) else {
        return Ok(Command::Help);
    };

    match command {
        "-h" | "--help" | "help" => {
            require_len(&arguments, 1, "help does not accept arguments")?;
            Ok(Command::Help)
        }
        "check" => {
            require_len(&arguments, 1, "check does not accept arguments")?;
            Ok(Command::Check)
        }
        "drift" => parse_drift(&arguments[1..]),
        "codegen" => parse_codegen(&arguments[1..]),
        "spec" => parse_spec(&arguments[1..]),
        other => Err(usage_error(format!("unknown command `{other}`"))),
    }
}

fn parse_drift(arguments: &[String]) -> Result<Command> {
    let mut from = None;
    let mut to = None;
    let mut index = 0;
    while index < arguments.len() {
        match arguments[index].as_str() {
            "--from" => {
                if from.is_some() {
                    return Err(usage_error("duplicate `--from` option"));
                }
                index += 1;
                from = Some(
                    arguments
                        .get(index)
                        .cloned()
                        .ok_or_else(|| usage_error("missing value for `--from`"))?,
                );
            }
            "--to" => {
                if to.is_some() {
                    return Err(usage_error("duplicate `--to` option"));
                }
                index += 1;
                to = Some(
                    arguments
                        .get(index)
                        .cloned()
                        .ok_or_else(|| usage_error("missing value for `--to`"))?,
                );
            }
            other => {
                return Err(usage_error(format!("unknown drift option `{other}`")));
            }
        }
        index += 1;
    }
    let to = to.ok_or_else(|| usage_error("drift requires `--to <path-or-spec>`"))?;
    Ok(Command::Drift(DriftArguments { from, to }))
}

fn parse_codegen(arguments: &[String]) -> Result<Command> {
    match arguments {
        [] => Ok(Command::Codegen { check: false }),
        [flag] if flag == "--check" => Ok(Command::Codegen { check: true }),
        _ => Err(usage_error(
            "codegen accepts only the optional `--check` flag",
        )),
    }
}

fn parse_spec(arguments: &[String]) -> Result<Command> {
    let Some(command) = arguments.first().map(String::as_str) else {
        return Err(usage_error("missing spec subcommand"));
    };

    match command {
        "verify" => {
            require_len(arguments, 1, "spec verify does not accept arguments")?;
            Ok(Command::Spec(SpecCommand::Verify))
        }
        "fetch" => parse_fetch(&arguments[1..]).map(|args| Command::Spec(SpecCommand::Fetch(args))),
        other => Err(usage_error(format!("unknown spec subcommand `{other}`"))),
    }
}

fn parse_fetch(arguments: &[String]) -> Result<FetchArguments> {
    let mut revision = None;
    let mut url = None;
    let mut index = 0;

    while index < arguments.len() {
        let flag = &arguments[index];
        index += 1;
        let value = arguments
            .get(index)
            .ok_or_else(|| usage_error(format!("missing value for spec fetch option `{flag}`")))?;
        index += 1;

        match flag.as_str() {
            "--rev" if revision.is_none() => revision = Some(value.clone()),
            "--url" if url.is_none() => url = Some(value.clone()),
            "--rev" | "--url" => {
                return Err(usage_error(format!(
                    "spec fetch option `{flag}` may be supplied only once"
                )));
            }
            _ => return Err(usage_error(format!("unknown spec fetch option `{flag}`"))),
        }
    }

    let revision =
        revision.ok_or_else(|| usage_error("spec fetch requires `--rev <40-char-sha>`"))?;
    Ok(FetchArguments { revision, url })
}

fn require_len(arguments: &[String], expected: usize, message: &str) -> Result<()> {
    if arguments.len() == expected {
        Ok(())
    } else {
        Err(usage_error(message))
    }
}

fn usage_error(message: impl Into<String>) -> Error {
    Error::message(format!("{}\n\n{}", message.into(), USAGE))
}

#[cfg(test)]
mod tests {
    use super::{Command, FetchArguments, SpecCommand, parse};

    #[test]
    fn parses_fetch() -> Result<(), Box<dyn std::error::Error>> {
        let command = parse([
            "spec".to_owned(),
            "fetch".to_owned(),
            "--rev".to_owned(),
            "a".repeat(40),
        ])?;
        assert_eq!(
            command,
            Command::Spec(SpecCommand::Fetch(FetchArguments {
                revision: "a".repeat(40),
                url: None,
            }))
        );
        Ok(())
    }

    #[test]
    fn parses_aggregate_check() -> Result<(), Box<dyn std::error::Error>> {
        assert_eq!(parse(["check".to_owned()])?, Command::Check);
        Ok(())
    }

    #[test]
    fn parses_drift() -> Result<(), Box<dyn std::error::Error>> {
        let command = parse([
            "drift".to_owned(),
            "--from".to_owned(),
            "spec/a.json".to_owned(),
            "--to".to_owned(),
            "spec/b.json".to_owned(),
        ])?;
        assert_eq!(
            command,
            Command::Drift(super::DriftArguments {
                from: Some("spec/a.json".to_owned()),
                to: "spec/b.json".to_owned(),
            })
        );
        Ok(())
    }

    #[test]
    fn rejects_duplicate_fetch_options() {
        let result = parse([
            "spec".to_owned(),
            "fetch".to_owned(),
            "--rev".to_owned(),
            "a".repeat(40),
            "--rev".to_owned(),
            "b".repeat(40),
        ]);
        assert!(result.is_err());
    }
}
