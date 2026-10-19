//! Project tasks: `cargo xtask <task>`.
//!
//! - `ci`: fmt check, clippy (warnings are errors), tests. Run before calling a phase done.
//! - `uninstall-desktop`: remove the per-user `.desktop` file, icon and MIME type that the
//!   former `install-desktop` put in `~/.local/share` (named `paintnotnet.*`, before the app ID).
//!   The package installs them system-wide under the app ID, which also names builds run from
//!   this checkout.
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
        Some("uninstall-desktop") => uninstall_desktop(),
        _ => {
            eprintln!("usage: cargo xtask ci | uninstall-desktop");
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

/// Where the former `install-desktop` put things: (desktop file, icon, MIME type for .pdn).
fn desktop_targets() -> Option<(std::path::PathBuf, std::path::PathBuf, std::path::PathBuf)> {
    let data = std::env::var_os("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
        })?;
    Some((
        data.join("applications/paintnotnet.desktop"),
        data.join("icons/hicolor/scalable/apps/paintnotnet.svg"),
        data.join("mime/packages/paintnotnet.xml"),
    ))
}

fn uninstall_desktop() -> ExitCode {
    let Some((desktop, icon, mime)) = desktop_targets() else {
        return ExitCode::FAILURE;
    };
    for path in [&desktop, &icon, &mime] {
        match std::fs::remove_file(path) {
            Ok(()) => eprintln!("removed {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => {
                eprintln!("{}: {e}", path.display());
                return ExitCode::FAILURE;
            }
        }
    }
    refresh_desktop_caches(&desktop, &mime);
    ExitCode::SUCCESS
}

/// Let the desktop notice new or removed entries (the tools are optional).
fn refresh_desktop_caches(desktop: &std::path::Path, mime: &std::path::Path) {
    if let Some(dir) = desktop.parent() {
        let _ = Command::new("update-desktop-database").arg(dir).status();
    }
    // `mime` is <data>/mime/packages/…: the database is <data>/mime.
    if let Some(dir) = mime.parent().and_then(|p| p.parent()) {
        let _ = Command::new("update-mime-database").arg(dir).status();
    }
    let _ = Command::new("kbuildsycoca6").status();
}
