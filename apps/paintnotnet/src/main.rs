//! paintnotnet: a paint.net 5.x remake for Linux.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use eframe::{SurfaceConfig, WgpuConfiguration, egui};
use paintnotnet::app::{PaintNotNetApp, SmokeTest};

fn main() -> ExitCode {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or(
            "info,wgpu_core=warn,wgpu_hal=warn,naga=warn,zbus=warn,tracing=warn",
        ),
    )
    .init();

    let (smoke, files) = match parse_args(std::env::args().skip(1)) {
        Ok(parsed) => parsed,
        Err(message) => {
            eprintln!("{message}\nusage: paintnotnet [--smoke-test SECONDS] [FILE]...");
            return ExitCode::from(2);
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("paintnotnet")
            // Wayland app_id: lets KDE match the .desktop file and window rules.
            .with_app_id("paintnotnet")
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([640.0, 480.0]),
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
                if let Some(path) = paintnotnet::config::default_path() {
                    let config = paintnotnet::config::Config::load(&path);
                    app.set_config(&cc.egui_ctx, config, Some(path));
                }
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

fn parse_args(
    mut args: impl Iterator<Item = String>,
) -> Result<(Option<SmokeTest>, Vec<PathBuf>), String> {
    let mut smoke = None;
    let mut files = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
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
    Ok((smoke, files))
}
