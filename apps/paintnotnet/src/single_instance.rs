//! One window: files opened while paintnotnet runs (from Dolphin, the launcher or a
//! terminal) open as tabs in the running window instead of starting another one, and a
//! launch without files just raises it.
//!
//! The first instance owns [`NAME`] on the session bus and serves `Open`. A later launch
//! finds the name taken, calls `Open` with its files (made absolute: the running window has
//! another working directory) and the activation token its launcher gave it, and exits; the
//! running window opens the files and asks the compositor to raise it with that token
//! (Wayland's xdg-activation: without the launcher's token KWin may refuse to give focus).
//! Without a session bus, or if the running window doesn't answer, the launch opens a
//! window of its own; `--new-window` always does.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, OnceLock};

use eframe::egui;

pub const NAME: &str = crate::APP_ID;
const PATH: &str = "/uk/co/maxwilson/paintnotnet";
const INTERFACE: &str = "uk.co.maxwilson.paintnotnet.Instance1";

/// Files handed over by another launch.
#[derive(Debug, PartialEq)]
pub struct Handoff {
    pub paths: Vec<PathBuf>,
    /// The other launch's `XDG_ACTIVATION_TOKEN`, if its launcher gave it one.
    pub activation_token: Option<String>,
}

struct Service {
    tx: Sender<Handoff>,
    ctx: Arc<OnceLock<egui::Context>>,
}

#[zbus::interface(name = "uk.co.maxwilson.paintnotnet.Instance1")]
impl Service {
    fn open(&self, paths: Vec<String>, activation_token: String) {
        let handoff = Handoff {
            paths: paths.into_iter().map(PathBuf::from).collect(),
            activation_token: (!activation_token.is_empty()).then_some(activation_token),
        };
        if self.tx.send(handoff).is_ok()
            && let Some(ctx) = self.ctx.get()
        {
            ctx.request_repaint();
        }
    }
}

/// The running window's end: holds the bus name while it lives.
pub struct Instance {
    _conn: zbus::blocking::Connection,
    rx: Receiver<Handoff>,
    ctx: Arc<OnceLock<egui::Context>>,
}

impl Instance {
    /// Repaint `ctx` when files arrive (they are picked up by the next frame).
    pub fn set_context(&self, ctx: &egui::Context) {
        let _ = self.ctx.set(ctx.clone());
    }

    /// Handoffs received since the last call.
    pub fn take(&self) -> Vec<Handoff> {
        self.rx.try_iter().collect()
    }
}

pub enum Start {
    /// This is the first window; it receives later launches' files.
    First(Instance),
    /// A running window took the files: exit.
    HandedOver,
    /// No session bus or no answer: run on our own.
    Alone,
}

/// Claim the bus name, or hand `files` to the window that has it.
pub fn start(files: &[PathBuf]) -> Start {
    start_as(NAME, files, std::env::var("XDG_ACTIVATION_TOKEN").ok())
}

fn start_as(name: &str, files: &[PathBuf], token: Option<String>) -> Start {
    let (tx, rx) = channel();
    let ctx = Arc::new(OnceLock::new());
    let service = Service {
        tx,
        ctx: ctx.clone(),
    };
    let conn = match zbus::blocking::connection::Builder::session()
        .and_then(|b| b.serve_at(PATH, service))
        .and_then(|b| b.build())
    {
        Ok(conn) => conn,
        Err(e) => {
            log::info!("one window: no session bus ({e}); opening a window of our own");
            return Start::Alone;
        }
    };
    use zbus::fdo::{RequestNameFlags, RequestNameReply};
    match conn.request_name_with_flags(name, RequestNameFlags::DoNotQueue.into()) {
        Ok(RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner) => {
            Start::First(Instance {
                _conn: conn,
                rx,
                ctx,
            })
        }
        // zbus reports a taken name as an error.
        Ok(_) | Err(zbus::Error::NameTaken) => match hand_over(&conn, name, files, token) {
            Ok(()) => Start::HandedOver,
            Err(e) => {
                log::warn!("one window: the running window didn't answer ({e})");
                Start::Alone
            }
        },
        Err(e) => {
            log::warn!("one window: {e}");
            Start::Alone
        }
    }
}

fn absolute(p: &Path) -> String {
    std::path::absolute(p)
        .unwrap_or_else(|_| p.to_owned())
        .to_string_lossy()
        .into_owned()
}

fn hand_over(
    conn: &zbus::blocking::Connection,
    name: &str,
    files: &[PathBuf],
    token: Option<String>,
) -> zbus::Result<()> {
    let paths: Vec<String> = files.iter().map(|p| absolute(p)).collect();
    conn.call_method(
        Some(name),
        PATH,
        Some(INTERFACE),
        "Open",
        &(paths, token.unwrap_or_default()),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two launches on the real session bus (under a test name): the second hands its files,
    /// made absolute, and its token to the first and exits. Skips without a session bus.
    #[test]
    fn second_launch_hands_files_to_the_first() {
        let name = format!("uk.co.maxwilson.paintnotnet.Test{}", std::process::id());
        let first = match start_as(&name, &[], None) {
            Start::First(i) => i,
            Start::Alone => {
                eprintln!("no session bus: skipped");
                return;
            }
            Start::HandedOver => panic!("nobody else has the test name"),
        };
        let files = [PathBuf::from("relative/a.png"), PathBuf::from("/abs/b.ora")];
        assert!(matches!(
            start_as(&name, &files, Some("token-1".into())),
            Start::HandedOver
        ));
        let got = first.take();
        let cwd = std::env::current_dir().expect("cwd");
        assert_eq!(
            got,
            vec![Handoff {
                paths: vec![cwd.join("relative/a.png"), PathBuf::from("/abs/b.ora")],
                activation_token: Some("token-1".into()),
            }]
        );
        // No files, no token: just a raise request.
        assert!(matches!(start_as(&name, &[], None), Start::HandedOver));
        assert_eq!(
            first.take(),
            vec![Handoff {
                paths: Vec::new(),
                activation_token: None,
            }]
        );
    }
}
