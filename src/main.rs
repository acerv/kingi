// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Andrea Cervesato <andrea.cervesato@suse.com>
mod core;
mod ui;

use core::config::Config;
use ui::app::App;

const USAGE: &str = "\
Usage:
  kingi                Start the email client
  kingi init [account] Initialize configuration for an account (default: \"default\")
  kingi -h | --help    Show this help message";

#[derive(Debug, PartialEq, Eq)]
enum CliAction {
    Run,
    Init(String),
    Help,
}

fn parse_args<I, S>(args: I) -> anyhow::Result<CliAction>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_string()).collect();
    match args.as_slice() {
        [] => Ok(CliAction::Run),
        [flag] if flag == "-h" || flag == "--help" => Ok(CliAction::Help),
        [cmd] if cmd == "init" => Ok(CliAction::Init("default".to_string())),
        [cmd, acct] if cmd == "init" && !acct.starts_with('-') => Ok(CliAction::Init(acct.clone())),
        _ => anyhow::bail!("invalid arguments\n\n{USAGE}"),
    }
}

fn main() -> anyhow::Result<()> {
    match parse_args(std::env::args().skip(1))? {
        CliAction::Run => {
            let config = Config::load()?;
            App::new(config)?.run()
        }
        CliAction::Init(account) => {
            let path = Config::init(&account)?;
            println!("Initialized account {account:?} at {}", path.display());
            Ok(())
        }
        CliAction::Help => {
            println!("{USAGE}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_args_empty_runs_tui() {
        assert_eq!(parse_args(Vec::<&str>::new()).unwrap(), CliAction::Run);
    }

    #[test]
    fn parse_args_init_defaults_to_default_account() {
        assert_eq!(
            parse_args(["init"]).unwrap(),
            CliAction::Init("default".to_string())
        );
    }

    #[test]
    fn parse_args_init_with_custom_account() {
        assert_eq!(
            parse_args(["init", "work"]).unwrap(),
            CliAction::Init("work".to_string())
        );
    }

    #[test]
    fn parse_args_help_flags() {
        assert_eq!(parse_args(["-h"]).unwrap(), CliAction::Help);
        assert_eq!(parse_args(["--help"]).unwrap(), CliAction::Help);
    }

    #[test]
    fn parse_args_rejects_unknown_arguments() {
        assert!(parse_args(["unknown"]).is_err());
        assert!(parse_args(["init", "a", "b"]).is_err());
        assert!(parse_args(["init", "-x"]).is_err());
    }
}
