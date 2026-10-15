//! The model runs in a helper process: the app starts itself again with a worker argument
//! and sends it images over a pipe.
//!
//! Why a process: Arch's `onnxruntime-opt-cuda` 1.29.0-3 ships a CUDA provider that calls
//! cuDNN without listing `libcudnn.so.9` among its dependencies, so loading it fails with
//! "undefined symbol: cudnnGetConvolutionBackwardDataAlgorithm_v7" unless cuDNN is already
//! loaded with its symbols visible. Starting the helper with `LD_PRELOAD=libcudnn.so.9` does
//! that without `unsafe` code. It also means unloading the model is ending the process, which
//! reliably gives its GPU memory back, and a driver crash can't take the editor down.
//!
//! Protocol (little-endian), one request and one reply at a time:
//! request `[quality u8][precision u8][path len u32][path][width u32][height u32][rgb]`;
//! reply `0 [device u8][microseconds u64][coverage]` or `1 [len u32][message]`.

use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Duration;

use crate::models::{Model, Precision, Quality};
use crate::runtime::{AiError, Device, Segmenter};

/// A matte from the worker.
pub struct Matte {
    pub coverage: Vec<u8>,
    pub device: Device,
    /// How long the model ran (not counting loading it).
    pub took: Duration,
}

/// The app's end: a running helper process.
pub struct Worker {
    child: Child,
    to: BufWriter<ChildStdin>,
    from: BufReader<ChildStdout>,
}

impl Drop for Worker {
    fn drop(&mut self) {
        // Closing its input ends the worker's loop; kill in case it's mid-run.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Worker {
    /// Start `exe arg` as the worker. With CUDA in use, cuDNN is preloaded (see above).
    pub fn spawn(exe: &Path, arg: &str, device: Device) -> std::io::Result<Worker> {
        let mut cmd = Command::new(exe);
        cmd.arg(arg)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        if device == Device::Cuda {
            let preload = match std::env::var("LD_PRELOAD") {
                Ok(p) if !p.is_empty() => format!("{p} libcudnn.so.9"),
                _ => "libcudnn.so.9".to_owned(),
            };
            cmd.env("LD_PRELOAD", preload);
        }
        let mut child = cmd.spawn()?;
        let to = BufWriter::new(child.stdin.take().expect("piped"));
        let from = BufReader::new(child.stdout.take().expect("piped"));
        Ok(Worker { child, to, from })
    }

    /// Run `model` (at `path`) on `rgb` (`w × h`).
    pub fn matte(
        &mut self,
        model: Model,
        path: &Path,
        rgb: &[u8],
        w: u32,
        h: u32,
    ) -> Result<Matte, AiError> {
        let io = |e: std::io::Error| AiError::Run(format!("the AI helper stopped ({e})"));
        let path = path.to_string_lossy();
        let to = &mut self.to;
        to.write_all(&[quality_code(model.quality), precision_code(model.precision)])
            .map_err(io)?;
        to.write_all(&(path.len() as u32).to_le_bytes())
            .map_err(io)?;
        to.write_all(path.as_bytes()).map_err(io)?;
        to.write_all(&w.to_le_bytes()).map_err(io)?;
        to.write_all(&h.to_le_bytes()).map_err(io)?;
        to.write_all(rgb).map_err(io)?;
        to.flush().map_err(io)?;
        let from = &mut self.from;
        match read_u8(from).map_err(io)? {
            0 => {
                let device = match read_u8(from).map_err(io)? {
                    0 => Device::Cuda,
                    _ => Device::Cpu,
                };
                let took = Duration::from_micros(read_u64(from).map_err(io)?);
                let mut coverage = vec![0u8; w as usize * h as usize];
                from.read_exact(&mut coverage).map_err(io)?;
                Ok(Matte {
                    coverage,
                    device,
                    took,
                })
            }
            _ => {
                let n = read_u32(from).map_err(io)? as usize;
                let mut msg = vec![0u8; n];
                from.read_exact(&mut msg).map_err(io)?;
                Err(AiError::Run(String::from_utf8_lossy(&msg).into_owned()))
            }
        }
    }
}

fn quality_code(q: Quality) -> u8 {
    match q {
        Quality::Best => 0,
        Quality::Fast => 1,
    }
}

fn precision_code(p: Precision) -> u8 {
    match p {
        Precision::Fp16 => 0,
        Precision::Fp32 => 1,
    }
}

fn read_u8(r: &mut impl Read) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u32(r: &mut impl Read) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(r: &mut impl Read) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

/// Largest image the worker accepts (pixels), so a corrupt request can't allocate wildly.
const MAX_PIXELS: u64 = 1 << 30;

/// The worker's end: answer requests on stdin until it closes.
pub fn serve() {
    let mut from = BufReader::new(std::io::stdin().lock());
    let mut to = BufWriter::new(std::io::stdout().lock());
    let mut loaded: Option<(PathBuf, Segmenter)> = None;
    loop {
        let quality = match read_u8(&mut from) {
            Ok(q) => q,
            Err(_) => return, // the app closed the pipe
        };
        // Outer error: the request couldn't be read (stop); inner: the model failed (reply).
        type Reply = Result<(Vec<u8>, Device, Duration), String>;
        let result = (|| -> std::io::Result<Reply> {
            let precision = read_u8(&mut from)?;
            let n = read_u32(&mut from)? as usize;
            if n > 4096 {
                return Err(std::io::Error::other("path too long"));
            }
            let mut path = vec![0u8; n];
            from.read_exact(&mut path)?;
            let path = PathBuf::from(String::from_utf8_lossy(&path).into_owned());
            let (w, h) = (read_u32(&mut from)?, read_u32(&mut from)?);
            if w as u64 * h as u64 > MAX_PIXELS {
                return Err(std::io::Error::other("image too large"));
            }
            let mut rgb = vec![0u8; w as usize * h as usize * 3];
            from.read_exact(&mut rgb)?;
            let model = Model::find(
                if quality == 0 {
                    Quality::Best
                } else {
                    Quality::Fast
                },
                if precision == 0 {
                    Precision::Fp16
                } else {
                    Precision::Fp32
                },
            );
            if loaded.as_ref().is_none_or(|(p, _)| *p != path) {
                loaded = None;
                let t = std::time::Instant::now();
                match Segmenter::load(model, &path) {
                    Ok(s) => {
                        log::info!(
                            "AI helper: loaded {} on {} in {:?}",
                            model.file_name(),
                            s.device.label(),
                            t.elapsed()
                        );
                        loaded = Some((path, s));
                    }
                    Err(e) => return Ok(Err(e.to_string())),
                }
            }
            let (_, seg) = loaded.as_mut().expect("loaded above");
            let t = std::time::Instant::now();
            Ok(seg
                .matte(&rgb, w as usize, h as usize)
                .map(|m| (m, seg.device, t.elapsed()))
                .map_err(|e| e.to_string()))
        })();
        let written = match result {
            Ok(Ok((coverage, device, took))) => (|| {
                to.write_all(&[0, if device == Device::Cuda { 0 } else { 1 }])?;
                to.write_all(&(took.as_micros() as u64).to_le_bytes())?;
                to.write_all(&coverage)?;
                to.flush()
            })(),
            Ok(Err(msg)) => (|| {
                to.write_all(&[1])?;
                to.write_all(&(msg.len() as u32).to_le_bytes())?;
                to.write_all(msg.as_bytes())?;
                to.flush()
            })(),
            Err(e) => {
                log::warn!("AI helper: bad request ({e})");
                return;
            }
        };
        if written.is_err() {
            return;
        }
    }
}
