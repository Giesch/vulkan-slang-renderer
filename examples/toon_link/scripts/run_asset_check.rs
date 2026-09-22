#!/usr/bin/env -S cargo +nightly -Zscript
---
[package]
edition = "2024"

[dependencies]
tempfile = "3"
---

//! Run an asset gate quietly on success, retaining diagnostics on failure.
//!
//! Test this script with:
//! ```sh
//! cargo +nightly -Zscript test --manifest-path examples/toon_link/scripts/run_asset_check.rs
//! ```

use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Seek, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

const HELP: &str = "Usage: run_asset_check.rs [--verbose | --quiet] -- COMMAND [ARGS...]\n\nRun an asset gate quietly on success, retaining diagnostics on failure.\n\n  --verbose  Stream all check output\n  --quiet    Only print failed check output (default)\n  -h, --help Show this help";

#[derive(Debug)]
struct Options {
    verbose: bool,
    command: Vec<OsString>,
}

fn parse_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<Options>, String> {
    let mut args = args.into_iter().peekable();
    let mut verbosity = None;
    while let Some(arg) = args.peek() {
        if arg == "--" {
            args.next();
            break;
        }

        if arg == "--help" || arg == "-h" {
            return Ok(None);
        }

        let verbose = if arg == "--verbose" {
            true
        } else if arg == "--quiet" {
            false
        } else {
            let unknown_option = arg.as_encoded_bytes().starts_with(b"-");
            if unknown_option {
                return Err(format!("unrecognized argument: {}", arg.to_string_lossy()));
            }

            break;
        };
        if verbosity.is_some_and(|previous| previous != verbose) {
            return Err("--verbose and --quiet cannot be used together".into());
        }

        verbosity = Some(verbose);
        args.next();
    }
    let command: Vec<_> = args.collect();
    if command.is_empty() {
        return Err("a check command is required".into());
    }

    Ok(Some(Options {
        verbose: verbosity.unwrap_or(false),
        command,
    }))
}

struct Outcome {
    code: i32,
    output: Option<File>,
}

impl Outcome {
    fn replay(&mut self, mut destination: impl Write) -> io::Result<()> {
        if let Some(output) = &mut self.output {
            output.rewind()?;
            io::copy(output, &mut destination)?;
            destination.flush()?;
        }

        Ok(())
    }
}

fn run_check(options: &Options, stdout: Stdio, stderr: Stdio) -> io::Result<Outcome> {
    let mut command = Command::new(&options.command[0]);
    command.args(&options.command[1..]);
    let output = if options.verbose {
        command.stdout(stdout).stderr(stderr);
        None
    } else {
        // Keep potentially large combined logs on disk, not in RAM.
        let output = tempfile::tempfile()?;
        command
            .stdout(output.try_clone()?)
            .stderr(output.try_clone()?);
        Some(output)
    };
    let status = command.status()?;
    let code = status
        .code()
        .unwrap_or_else(|| 128 + status.signal().unwrap_or(1));

    Ok(Outcome {
        code,
        output: if status.success() { None } else { output },
    })
}

fn main() {
    let options = match parse_args(std::env::args_os().skip(1)) {
        Ok(Some(options)) => options,
        Ok(None) => {
            println!("{HELP}");
            return;
        }
        Err(error) => {
            eprintln!("asset check: {error}\n\n{HELP}");
            std::process::exit(2);
        }
    };
    let result = run_check(&options, Stdio::inherit(), Stdio::inherit()).and_then(|mut outcome| {
        outcome.replay(io::stderr().lock())?;

        Ok(outcome.code)
    });
    let code = result.unwrap_or_else(|error| {
        eprintln!("asset check: {error}");

        1
    });
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn parse(args: &[&str]) -> Result<Option<Options>, String> {
        parse_args(args.iter().map(OsString::from))
    }

    fn read(file: &mut File) -> Vec<u8> {
        file.rewind().unwrap();
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes).unwrap();

        bytes
    }

    fn check(flags: &[&str], script: &str) -> (i32, Vec<u8>, Vec<u8>) {
        let mut args = flags.to_vec();
        args.extend(["--", "sh", "-c", script]);
        let options = parse(&args).unwrap().unwrap();
        let mut stdout = tempfile::tempfile().unwrap();
        let mut stderr = tempfile::tempfile().unwrap();
        let mut outcome = run_check(
            &options,
            stdout.try_clone().unwrap().into(),
            stderr.try_clone().unwrap().into(),
        )
        .unwrap();
        outcome.replay(&mut stderr).unwrap();

        (outcome.code, read(&mut stdout), read(&mut stderr))
    }

    #[test]
    fn success_is_silent_by_default_and_explicitly() {
        for flags in [&[][..], &["--quiet"][..]] {
            assert_eq!(
                check(flags, "echo progress; echo log >&2"),
                (0, vec![], vec![])
            );
        }
    }

    #[test]
    fn verbose_preserves_both_streams() {
        for code in [0, 7] {
            assert_eq!(
                check(
                    &["--verbose"],
                    &format!("echo progress; echo log >&2; exit {code}")
                ),
                (code, b"progress\n".to_vec(), b"log\n".to_vec())
            );
        }
    }

    #[test]
    fn failure_replays_output_and_preserves_status() {
        assert_eq!(
            check(&[], "echo context; echo failure >&2; exit 7"),
            (7, vec![], b"context\nfailure\n".to_vec())
        );
    }

    #[test]
    fn invalid_options_and_missing_commands_fail() {
        for args in [
            &[][..],
            &["--"][..],
            &["--quiet"][..],
            &["--verbsoe", "--", "sh"][..],
            &["--verbose", "--quiet", "--", "sh"][..],
            &["--quiet", "--verbose", "--", "sh"][..],
        ] {
            assert!(parse(args).is_err(), "{args:?}");
        }
    }

    #[test]
    fn help_and_command_arguments() {
        assert!(parse(&["--help"]).unwrap().is_none());
        assert!(parse(&["-h"]).unwrap().is_none());
        for args in [&["--", "echo", "--verbose"][..], &["echo", "--verbose"][..]] {
            let options = parse(args).unwrap().unwrap();
            assert!(!options.verbose);
            assert_eq!(options.command, ["echo", "--verbose"]);
        }
    }

    #[test]
    fn spawn_failure_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let options = Options {
            verbose: false,
            command: vec![directory.path().join("missing-command").into_os_string()],
        };
        let error = run_check(&options, Stdio::null(), Stdio::null())
            .err()
            .unwrap();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn signal_status_is_preserved() {
        assert_eq!(
            check(&[], "echo interrupted; kill -TERM $$"),
            (143, vec![], b"interrupted\n".to_vec())
        );
    }

    #[test]
    fn large_failure_logs_are_replayed_completely() {
        let (code, stdout, stderr) = check(&[], "head -c 200000 /dev/zero; exit 9");
        assert_eq!(code, 9);
        assert!(stdout.is_empty());
        assert_eq!(stderr, vec![0; 200_000]);
    }
}
