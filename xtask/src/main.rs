//! Project tasks: `cargo xtask <task>`.
//!
//! - `ci`: fmt check, clippy (warnings are errors), tests. Run before calling a phase done.
//! - `install-desktop [--force]`: install the `.desktop` file and icon for this user
//!   (`~/.local/share`), so the desktop shows paintnotnet's icon and name for a build run
//!   from this checkout (the package installs them system-wide). `uninstall-desktop` removes
//!   them again.
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
        Some("install-desktop") => install_desktop(std::env::args().any(|a| a == "--force")),
        Some("uninstall-desktop") => uninstall_desktop(),
        _ => {
            eprintln!("usage: cargo xtask ci | install-desktop [--force] | uninstall-desktop");
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

/// Where `install-desktop` puts things: (desktop file, icon, MIME type for .pdn).
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

fn install_desktop(force: bool) -> ExitCode {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let Some((desktop, icon, mime)) = desktop_targets() else {
        eprintln!("no HOME or XDG_DATA_HOME");
        return ExitCode::FAILURE;
    };
    // Run the checkout's release build if there is one, else the debug build.
    let binary = ["release", "debug"]
        .iter()
        .map(|p| root.join("target").join(p).join("paintnotnet"))
        .find(|b| b.exists())
        .unwrap_or_else(|| root.join("target/release/paintnotnet"));
    let binary = binary.canonicalize().unwrap_or(binary);
    let entry = match std::fs::read_to_string(root.join("packaging/paintnotnet.desktop")) {
        Ok(text) => text.replace(
            "Exec=paintnotnet %F",
            &format!("Exec=\"{}\" %F", binary.display()),
        ),
        Err(e) => {
            eprintln!("packaging/paintnotnet.desktop: {e}");
            return ExitCode::FAILURE;
        }
    };
    let svg = match std::fs::read(root.join("packaging/paintnotnet.svg")) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("packaging/paintnotnet.svg: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mime_xml = match std::fs::read(root.join("packaging/paintnotnet-mime.xml")) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("packaging/paintnotnet-mime.xml: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (path, contents) in [
        (&desktop, entry.into_bytes()),
        (&icon, svg),
        (&mime, mime_xml),
    ] {
        if path.exists() && std::fs::read(path).ok().as_deref() != Some(&contents[..]) && !force {
            eprintln!(
                "{} exists and differs; not overwriting (use --force)",
                path.display()
            );
            return ExitCode::FAILURE;
        }
        if let Some(dir) = path.parent()
            && let Err(e) = std::fs::create_dir_all(dir)
        {
            eprintln!("{}: {e}", dir.display());
            return ExitCode::FAILURE;
        }
        if let Err(e) = std::fs::write(path, &contents) {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
        eprintln!("installed {}", path.display());
    }
    refresh_desktop_caches(&desktop, &mime);
    eprintln!("Exec points at {}", binary.display());
    ExitCode::SUCCESS
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
