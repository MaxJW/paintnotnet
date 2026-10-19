//! Remove Background and Select Subject: finding an image's subject with a local AI model
//! (`pnn-ai`, BiRefNet on ONNX Runtime), in the background.
//!
//! A [`MatteJob`] goes from asking to download the model (nothing is fetched until the user
//! agrees), through downloading and running it, to a matte: the subject's coverage, 0–255
//! per pixel. The model runs in a helper process (paintnotnet started with `--ai-worker`, see
//! `pnn_ai::worker`), kept for a while after use (loading takes seconds) and ended when idle,
//! which frees its GPU memory; mattes are cached by the pixels they came from, so changing
//! settings or running Select Subject after Remove Background on the same pixels doesn't run
//! the model again.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui;
use pnn_ai::{Device, Model, Progress, Quality, Worker};
use pnn_core::Surface;

/// Free the model (and the GPU memory it holds) after this long unused.
const KEEP_LOADED: Duration = Duration::from_secs(120);
/// Mattes kept (each is one byte per pixel).
const CACHED: usize = 4;

/// The subject's coverage of an image.
pub struct Matte {
    pub width: u32,
    pub height: u32,
    pub coverage: Vec<u8>,
    pub model: Model,
    pub device: Device,
    /// How long the model took (not counting loading it).
    pub took: Duration,
    /// The subject's and the background's own colours where the matte is soft (Remove
    /// Background's edge clean-up), worked out once, on the job's thread, when first needed.
    pub colours: std::sync::OnceLock<pnn_ai::EdgeColours>,
}

type CacheKey = (u64, u32, u32, Quality);

/// The argument that makes paintnotnet run as the AI helper (handled first thing in `main`).
pub const WORKER_ARG: &str = "--ai-worker";

#[derive(Default)]
struct Shared {
    worker: Option<Worker>,
    cache: Vec<(CacheKey, Arc<Matte>)>,
    last_used: Option<Instant>,
}

/// The app's AI state: one loaded model at a time and recent mattes.
pub struct Ai {
    shared: Arc<Mutex<Shared>>,
    dir: PathBuf,
    /// The program started as the helper (this one; tests point it at the built app).
    worker_exe: PathBuf,
}

impl Default for Ai {
    fn default() -> Self {
        Self::with_dir(pnn_ai::models_dir())
    }
}

impl Ai {
    /// Keep models in `dir` instead (tests).
    pub fn with_dir(dir: PathBuf) -> Self {
        Self {
            shared: Default::default(),
            dir,
            worker_exe: std::env::current_exe().unwrap_or_else(|_| "paintnotnet".into()),
        }
    }

    /// Start `exe` as the helper (tests: the built app rather than the test binary).
    pub fn set_worker_exe(&mut self, exe: PathBuf) {
        self.worker_exe = exe;
    }

    pub fn models_dir(&self) -> &std::path::Path {
        &self.dir
    }

    /// The model file for `quality` on this computer's device.
    pub fn model(&self, quality: Quality) -> Result<Model, String> {
        let rt = pnn_ai::runtime().map_err(|e| e.to_string())?;
        Ok(Model::find(quality, rt.device.precision()))
    }

    pub fn device(&self) -> Option<Device> {
        pnn_ai::runtime().ok().map(|rt| rt.device)
    }

    /// Best if it's downloaded, else fast if that is; with neither, best on a GPU and fast
    /// on the CPU (where best takes many seconds).
    pub fn default_quality(&self) -> Quality {
        let present = |q| self.model(q).is_ok_and(|m| m.is_present_in(&self.dir));
        if present(Quality::Best) {
            Quality::Best
        } else if present(Quality::Fast) {
            Quality::Fast
        } else if self.device() == Some(Device::Cuda) {
            Quality::Best
        } else {
            Quality::Fast
        }
    }

    /// Unload the model once it has been idle a while (called every frame).
    pub fn tick(&self) {
        if let Ok(mut s) = self.shared.try_lock()
            && s.worker.is_some()
            && s.last_used.is_some_and(|t| t.elapsed() > KEEP_LOADED)
        {
            log::info!("AI: ending the idle helper");
            s.worker = None;
        }
    }

    /// Unload now (exit).
    pub fn unload(&self) {
        if let Ok(mut s) = self.shared.lock() {
            s.worker = None;
        }
    }

    /// Start finding the subject of `source`; with `colours`, also its edge colours (the job
    /// is ready once both are).
    pub fn start(
        &self,
        ctx: &egui::Context,
        source: Surface,
        quality: Quality,
        colours: bool,
    ) -> MatteJob {
        let mut job = MatteJob {
            quality,
            colours,
            source,
            stage: Stage::Working,
            rx: None,
            progress: None,
        };
        job.begin(self, ctx);
        job
    }
}

pub enum Stage {
    /// Waiting for the user to agree to download the model.
    Consent(Model),
    Downloading(Model),
    /// Loading and running the model.
    Working,
    Ready(Arc<Matte>),
    Failed(String),
}

enum Msg {
    Downloaded(Result<(), String>),
    Done(Result<Arc<Matte>, String>),
}

/// Finding one image's subject.
pub struct MatteJob {
    pub quality: Quality,
    /// Work out the edge colours too.
    colours: bool,
    source: Surface,
    pub stage: Stage,
    rx: Option<Receiver<Msg>>,
    progress: Option<Arc<Progress>>,
}

impl Drop for MatteJob {
    fn drop(&mut self) {
        if let Some(p) = &self.progress {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

impl MatteJob {
    pub fn matte(&self) -> Option<&Arc<Matte>> {
        match &self.stage {
            Stage::Ready(m) => Some(m),
            _ => None,
        }
    }

    /// Bytes downloaded so far.
    pub fn received(&self) -> u64 {
        self.progress
            .as_ref()
            .map_or(0, |p| p.received.load(std::sync::atomic::Ordering::Relaxed))
    }

    /// Switch model (best / fast) and start again.
    pub fn set_quality(&mut self, ai: &Ai, ctx: &egui::Context, quality: Quality) {
        if quality != self.quality {
            self.quality = quality;
            self.begin(ai, ctx);
        }
    }

    fn begin(&mut self, ai: &Ai, ctx: &egui::Context) {
        self.cancel_download();
        self.rx = None;
        let model = match ai.model(self.quality) {
            Ok(m) => m,
            Err(e) => {
                self.stage = Stage::Failed(e);
                return;
            }
        };
        if model.is_present_in(&ai.dir) {
            self.run(ai, ctx, model);
        } else {
            self.stage = Stage::Consent(model);
        }
    }

    fn cancel_download(&mut self) {
        if let Some(p) = self.progress.take() {
            p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// The user agreed: download the model, then run it.
    pub fn download(&mut self, ai: &Ai, ctx: &egui::Context) {
        let Stage::Consent(model) = self.stage else {
            return;
        };
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.stage = Stage::Downloading(model);
        let (dir, ctx) = (ai.dir.clone(), ctx.clone());
        std::thread::spawn(move || {
            // Repaint while it downloads, so the progress bar moves.
            let ticker = {
                let (ctx, progress) = (ctx.clone(), progress.clone());
                std::thread::spawn(move || {
                    while Arc::strong_count(&progress) > 2 {
                        ctx.request_repaint();
                        std::thread::sleep(Duration::from_millis(200));
                    }
                })
            };
            let result = pnn_ai::download(&model, &dir, &progress)
                .map(|_| ())
                .map_err(|e| e.to_string());
            drop(progress);
            let _ = tx.send(Msg::Downloaded(result));
            ctx.request_repaint();
            let _ = ticker.join();
        });
    }

    fn run(&mut self, ai: &Ai, ctx: &egui::Context, model: Model) {
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.stage = Stage::Working;
        let (shared, source, ctx) = (ai.shared.clone(), self.source.clone(), ctx.clone());
        let path = model.path_in(&ai.dir);
        let exe = ai.worker_exe.clone();
        let colours = self.colours;
        std::thread::spawn(move || {
            let result = find_subject(&shared, &source, model, &path, &exe).map(|(m, rgb)| {
                if colours {
                    m.colours.get_or_init(|| {
                        let t = Instant::now();
                        let c = pnn_ai::foreground::estimate(
                            &rgb,
                            &m.coverage,
                            m.width as usize,
                            m.height as usize,
                        );
                        log::info!("AI: edge colours took {:?}", t.elapsed());
                        c
                    });
                }
                m
            });
            let _ = tx.send(Msg::Done(result));
            ctx.request_repaint();
        });
    }

    /// Pick up results from the background; true if the stage changed.
    pub fn poll(&mut self, ai: &Ai, ctx: &egui::Context) -> bool {
        let Some(msg) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return false;
        };
        match msg {
            Msg::Downloaded(Ok(())) => {
                self.progress = None;
                let model = match self.stage {
                    Stage::Downloading(m) => m,
                    _ => return true,
                };
                self.run(ai, ctx, model);
            }
            Msg::Downloaded(Err(e)) | Msg::Done(Err(e)) => {
                self.progress = None;
                self.rx = None;
                self.stage = Stage::Failed(e);
            }
            Msg::Done(Ok(m)) => {
                self.rx = None;
                self.stage = Stage::Ready(m);
            }
        }
        true
    }
}

/// `source` over white as RGB (the models were trained on opaque photos).
fn rgb_over_white(source: &Surface) -> Vec<u8> {
    use rayon::prelude::*;
    source
        .to_pixels()
        .par_iter()
        .flat_map_iter(|c| {
            let a = c.a as u32;
            let over = |v: u8| ((v as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            [over(c.r), over(c.g), over(c.b)]
        })
        .collect()
}

fn find_subject(
    shared: &Mutex<Shared>,
    source: &Surface,
    model: Model,
    path: &std::path::Path,
    exe: &std::path::Path,
) -> Result<(Arc<Matte>, Vec<u8>), String> {
    use std::hash::{Hash, Hasher};
    let (w, h) = (source.width(), source.height());
    let rgb = rgb_over_white(source);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    rgb.hash(&mut hasher);
    let key = (hasher.finish(), w, h, model.quality);
    let mut s = shared
        .lock()
        .map_err(|_| "the AI state was poisoned".to_owned())?;
    s.last_used = Some(Instant::now());
    if let Some((_, m)) = s.cache.iter().find(|(k, _)| *k == key) {
        return Ok((m.clone(), rgb));
    }
    let device = pnn_ai::runtime().map_err(|e| e.to_string())?.device;
    // A helper that died (or a first run): start one. One retry if it stops mid-request.
    let mut attempt = 0;
    let got = loop {
        if s.worker.is_none() {
            s.worker = Some(
                Worker::spawn(exe, WORKER_ARG, device)
                    .map_err(|e| format!("couldn't start the AI helper: {e}"))?,
            );
        }
        let worker = s.worker.as_mut().expect("started above");
        let t = Instant::now();
        match worker.matte(model, path, &rgb, w, h) {
            Ok(m) => break (m, t.elapsed()),
            Err(pnn_ai::AiError::Run(e))
                if e.starts_with("the AI helper stopped") && attempt == 0 =>
            {
                log::warn!("AI: {e}; starting it again");
                s.worker = None;
                attempt += 1;
            }
            Err(e) => {
                s.worker = None;
                return Err(e.to_string());
            }
        }
    };
    let (m, total) = got;
    let matte = Arc::new(Matte {
        width: w,
        height: h,
        coverage: m.coverage,
        model,
        device: m.device,
        took: m.took,
        colours: Default::default(),
    });
    log::info!(
        "AI: {} on {} took {:?} ({:?} with loading and transfer) for {w}×{h}",
        model.name(),
        m.device.label(),
        matte.took,
        total
    );
    s.cache.push((key, matte.clone()));
    if s.cache.len() > CACHED {
        s.cache.remove(0);
    }
    s.last_used = Some(Instant::now());
    Ok((matte, rgb))
}

/// A file in the models folder, or a model that could be there.
pub struct ModelFile {
    pub label: String,
    pub path: PathBuf,
    /// Bytes on disk; `None` = not downloaded.
    pub size: Option<u64>,
}

/// The models we can use (downloaded or not), then anything else in `dir`: older versions,
/// unfinished downloads.
pub fn model_files(dir: &std::path::Path) -> Vec<ModelFile> {
    let mut out: Vec<ModelFile> = pnn_ai::MODELS
        .iter()
        .map(|m| {
            let path = m.path_in(dir);
            let size = std::fs::metadata(&path).ok().map(|md| md.len());
            let precision = match m.precision {
                pnn_ai::Precision::Fp16 => "for the GPU",
                pnn_ai::Precision::Fp32 => "for the CPU",
            };
            let mut label = format!("{} ({precision})", m.name());
            if size.is_some_and(|s| s != m.size) {
                label.push_str(", incomplete");
            }
            ModelFile { label, path, size }
        })
        .collect();
    let mut others: Vec<ModelFile> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter(|e| !out.iter().any(|f| f.path == e.path()))
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let label = if name.ends_with(".part") {
                format!("{name} (unfinished download)")
            } else {
                format!("{name} (not used)")
            };
            ModelFile {
                label,
                path: e.path(),
                size: e.metadata().ok().map(|m| m.len()),
            }
        })
        .collect();
    others.sort_by(|a, b| a.label.cmp(&b.label));
    out.extend(others);
    out
}

/// `dir` with the home folder as `~`.
pub fn tilde(dir: &std::path::Path) -> String {
    match std::env::var_os("HOME").and_then(|h| dir.strip_prefix(h).ok().map(|p| p.to_owned())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => dir.display().to_string(),
    }
}

/// Human-readable size: "115 MB".
pub fn megabytes(bytes: u64) -> String {
    format!("{} MB", (bytes as f64 / 1e6).round())
}

/// The status part of the Remove Background and Select Subject dialogs: the download
/// question, progress, errors. Returns true if the user pressed Download.
pub fn status_ui(ui: &mut egui::Ui, job: &MatteJob, dir: &std::path::Path) -> bool {
    let mut download = false;
    match &job.stage {
        Stage::Consent(model) => {
            ui.label(format!(
                "This uses {}, an AI model that runs on this computer (MIT licence). It needs \
                 a one-time download of {} from Hugging Face ({}), checked and kept in {}. \
                 Your image never leaves this computer.",
                model.name(),
                megabytes(model.size),
                model.repo,
                tilde(dir)
            ));
            ui.add_space(4.0);
            download = ui
                .button(format!("Download {}", megabytes(model.size)))
                .clicked();
        }
        Stage::Downloading(model) => {
            let got = job.received();
            ui.label(format!(
                "Downloading {}: {} of {}",
                model.name(),
                megabytes(got),
                megabytes(model.size)
            ));
            ui.add(egui::ProgressBar::new(
                got as f32 / model.size.max(1) as f32,
            ));
        }
        Stage::Working => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Finding the subject…");
            });
        }
        Stage::Ready(m) => {
            ui.weak(format!(
                "{} on the {}, {:.1} s",
                m.model.name(),
                m.device.label(),
                m.took.as_secs_f32()
            ));
        }
        Stage::Failed(e) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
    }
    download
}

/// Edit › Select Subject in progress: the image as it looks (all visible layers).
pub struct SubjectSession {
    key: u64,
    job: MatteJob,
}

impl crate::app::PaintNotNetApp {
    /// Edit › Select Subject: find the subject of the visible image and select it.
    pub(crate) fn select_subject(&mut self, ctx: &egui::Context) {
        let Some(tab) = self.workspace.active() else {
            return;
        };
        let image = pnn_core::flatten(&tab.doc);
        let job = self.ai.start(ctx, image, self.ai.default_quality(), false);
        self.select_subject = Some(SubjectSession { key: tab.key, job });
        // With the model at hand and the image cached this finishes at once.
        self.run_select_subject(ctx);
    }

    /// Show Select Subject's progress; select once the matte is ready.
    pub(crate) fn run_select_subject(&mut self, ctx: &egui::Context) {
        let Some(s) = self.select_subject.as_mut() else {
            return;
        };
        s.job.poll(&self.ai, ctx);
        if let Some(m) = s.job.matte().cloned() {
            let key = s.key;
            self.select_subject = None;
            if let Some(tab) = self.workspace.tab_mut(key)
                && tab.doc.width() == m.width
                && tab.doc.height() == m.height
            {
                let selection = pnn_core::Selection::from_coverage(&m.coverage, m.width, m.height);
                if selection.is_empty() {
                    self.errors.push((
                        "Select Subject".into(),
                        "No subject was found in this image.".into(),
                    ));
                } else {
                    tab.doc.set_selection(selection);
                    tab.commit("Select Subject", crate::icons::Icon::SelectSubject);
                }
            }
            return;
        }
        let mut close = false;
        let mut download = false;
        let mut quality = s.job.quality;
        let dir = self.ai.models_dir().to_owned();
        crate::dialogs::movable_dialog(
            ctx,
            egui::Window::new("Select Subject")
                .id(egui::Id::new("select_subject"))
                .collapsible(false)
                .resizable(false)
                .pivot(egui::Align2::CENTER_CENTER)
                .default_pos(ctx.content_rect().center()),
            self.icons.art(crate::icons::Icon::SelectSubject),
            "Select Subject",
            None,
            |ui| {
                ui.set_max_width(360.0);
                if matches!(s.job.stage, Stage::Consent(_)) {
                    ui.label("Quality");
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut quality, Quality::Best, "Best");
                        ui.selectable_value(&mut quality, Quality::Fast, "Fast");
                    });
                    ui.add_space(4.0);
                }
                download = status_ui(ui, &s.job, &dir);
                ui.add_space(6.0);
                close = ui.button("Cancel").clicked();
            },
        );
        close |= ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if close {
            self.select_subject = None;
            return;
        }
        if quality != s.job.quality {
            s.job.set_quality(&self.ai, ctx, quality);
        }
        if download {
            s.job.download(&self.ai, ctx);
        }
    }
}
