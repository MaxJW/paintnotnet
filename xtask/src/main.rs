//! Project tasks: `cargo xtask <task>`.
//!
//! - `ci`: fmt check, clippy (warnings are errors), tests. Run before calling a phase done.
//!
//! CI runs at low CPU and I/O priority with a few cores left free, so the desktop stays
//! responsive while it builds. `PNN_CI_JOBS` overrides the job count.

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
    let cpus = std::thread::available_parallelism().map_or(4, |n| n.get());
    let jobs = std::env::var("PNN_CI_JOBS")
        .ok()
        .and_then(|j| j.parse::<usize>().ok())
        .unwrap_or_else(|| cpus.saturating_sub(4).max(1));
    for step in CI_STEPS {
        eprintln!("==> cargo {} ({jobs} jobs, low priority)", step.join(" "));
        match low_priority(&cargo)
            .args(*step)
            .env("CARGO_BUILD_JOBS", jobs.to_string())
            .env("RUST_TEST_THREADS", jobs.to_string())
            .status()
        {
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

/// `cargo` under `nice -n 15 ionice -c 3` when those tools exist (util-linux, coreutils).
fn low_priority(cargo: &str) -> Command {
    let have = |tool: &str| {
        Command::new(tool)
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
    };
    let mut cmd = if have("nice") {
        let mut c = Command::new("nice");
        c.args(["-n", "15"]);
        c
    } else {
        return Command::new(cargo);
    };
    if have("ionice") {
        cmd.args(["ionice", "-c", "3"]);
    }
    cmd.arg(cargo);
    cmd
}
