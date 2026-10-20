//! paintnotnet: a paint.net 5.x remake for Linux.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use eframe::{SurfaceConfig, WgpuConfiguration, egui};
use paintnotnet::app::{PaintNotNetApp, SmokeTest};

fn main() -> ExitCode {
    // The session log file, except for the AI helper and smoke tests (which would push the
    // user's last session aside).
    let helper = std::env::args().nth(1).as_deref() == Some(paintnotnet::ai::WORKER_ARG);
    let smoke_arg = std::env::args().any(|a| a == "--smoke-test");
    paintnotnet::session_log::init(!helper && !smoke_arg);

    // The AI helper process (`ai.rs`): answer model requests on stdin, nothing else.
    if std::env::args().nth(1).as_deref() == Some(paintnotnet::ai::WORKER_ARG) {
        pnn_ai::worker::serve();
        return ExitCode::SUCCESS;
    }

    let Args {
        smoke,
        files,
        new_window,
    } = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!(
                "{message}\nusage: paintnotnet [--new-window] [--smoke-test SECONDS] [FILE]..."
            );
            return ExitCode::from(2);
        }
    };
    // One window: a running paintnotnet takes the files. Smoke tests run alone unless
    // PNN_SMOKE_ONE_WINDOW=1 (to test the handoff with a window that closes itself).
    let smoke_alone = smoke.is_some() && std::env::var_os("PNN_SMOKE_ONE_WINDOW").is_none();
    let instance = if new_window || smoke_alone {
        None
    } else {
        match paintnotnet::single_instance::start(&files) {
            paintnotnet::single_instance::Start::HandedOver => return ExitCode::SUCCESS,
            paintnotnet::single_instance::Start::First(i) => Some(i),
            paintnotnet::single_instance::Start::Alone => None,
        }
    };

    // The settings, read before the window opens (it needs them to choose its title bar).
    let config = (smoke.is_none())
        .then(paintnotnet::config::default_path)
        .flatten()
        .map(|path| (paintnotnet::config::Config::load(&path), path));
    let system_title_bar = config.as_ref().is_some_and(|(c, _)| c.system_title_bar);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("paintnotnet")
            // paintnotnet's own window buttons sit in its title bar (unless set otherwise).
            .with_decorations(system_title_bar)
            // Wayland app_id: lets KDE match the .desktop file and window rules.
            .with_app_id(paintnotnet::APP_ID)
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0])
            // X11 window icon; on Wayland the desktop shows the .desktop file's icon.
            .with_icon(
                eframe::icon_data::from_png_bytes(include_bytes!(
                    "../../../packaging/paintnotnet-256.png"
                ))
                .unwrap_or_default(),
            ),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: WgpuConfiguration {
            // Brush strokes should land on screen as early as possible.
            surface: SurfaceConfig::LOW_LATENCY,
            ..Default::default()
        },
        ..Default::default()
    };

    let result = eframe::run_native(
        "paintnotnet",
        options,
        Box::new(move |cc| {
            let smoke_run = smoke.is_some();
            let mut app = PaintNotNetApp::new(cc, smoke, files);
            if !smoke_run {
                // Autosave for crash recovery, then the user's settings (which may change
                // its interval). Smoke tests use neither.
                if let Some(dir) = paintnotnet::autosave::default_state_dir() {
                    app.enable_autosave(dir, std::time::Duration::from_secs(60));
                }
                app.enable_watchdog();
                if let Some((config, path)) = config {
                    app.set_config(&cc.egui_ctx, config, Some(path));
                }
                app.set_recent_thumbs_dir(paintnotnet::recent_thumbs::default_dir());
                app.preload_fonts();
                let link = paintnotnet::wayland::WindowLink::new(cc);
                let menu = paintnotnet::global_menu::GlobalMenu::new(cc, link.as_ref());
                app.set_window_link(link);
                app.set_global_menu(menu);
            }
            if let Some(i) = instance {
                i.set_context(&cc.egui_ctx);
                app.set_instance(i);
            }
            Ok(Box::new(app))
        }),
    );
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            log::error!("{err}");
            ExitCode::FAILURE
        }
    }
}

struct Args {
    smoke: Option<SmokeTest>,
    files: Vec<PathBuf>,
    /// Open a window of our own even if paintnotnet is already running.
    new_window: bool,
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut smoke = None;
    let mut files = Vec::new();
    let mut new_window = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--new-window" => new_window = true,
            "--smoke-test" => {
                let value = args
                    .next()
                    .ok_or("--smoke-test needs a duration in seconds")?;
                let secs: f64 = value
                    .parse()
                    .ok()
                    .filter(|s: &f64| s.is_finite() && *s > 0.0)
                    .ok_or_else(|| format!("invalid --smoke-test duration: {value}"))?;
                smoke = Some(SmokeTest::new(Duration::from_secs_f64(secs)));
            }
            flag if flag.starts_with("--") => return Err(format!("unknown argument: {flag}")),
            file => files.push(PathBuf::from(file)),
        }
    }
    Ok(Args {
        smoke,
        files,
        new_window,
    })
}
