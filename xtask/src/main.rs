//! Project tasks: `cargo xtask <task>`.
//!
//! - `ci`: fmt check, clippy (warnings are errors), tests. Run before calling a phase done.

use std::process::{Command, ExitCode};

const CI_STEPS: &[&[&str]] = &[
    &["fmt", "--all", "--check"],
    &[
        "clippy",
        "--workspace",
        "--all-targets",
        "--",
        "-D",
        "warnings",
    ],
    &["test", "--workspace"],
];

fn main() -> ExitCode {
    match std::env::args().nth(1).as_deref() {
        Some("ci") => ci(),
        _ => {
            eprintln!("usage: cargo xtask ci");
            ExitCode::from(2)
        }
    }
}

fn ci() -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    for step in CI_STEPS {
        eprintln!("==> cargo {}", step.join(" "));
        match Command::new(&cargo).args(*step).status() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("FAILED: cargo {} ({status})", step.join(" "));
                return ExitCode::FAILURE;
            }
            Err(err) => {
                eprintln!("could not run cargo: {err}");
                return ExitCode::FAILURE;
            }
        }
    }
    eprintln!("==> all checks passed");
    ExitCode::SUCCESS
}
