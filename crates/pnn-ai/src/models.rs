//! The models we can run, where they live, and fetching them.
//!
//! BiRefNet (Zheng et al., "Bilateral Reference for High-Resolution Dichotomous Image
//! Segmentation", CAAI AIR 2024; MIT) as exported to ONNX by onnx-community on Hugging Face,
//! pinned to one revision each. Sizes and SHA-256 sums are the ones Hugging Face lists for
//! those revisions' LFS files; a download that doesn't match is thrown away.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use sha2::{Digest, Sha256};

/// Best (BiRefNet) or fast (BiRefNet lite).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Quality {
    Best,
    Fast,
}

/// Half precision for the GPU, single for the CPU (where half is slower, not faster).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Precision {
    Fp16,
    Fp32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Model {
    pub quality: Quality,
    pub precision: Precision,
    pub repo: &'static str,
    pub revision: &'static str,
    /// Path inside the repository.
    pub file: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

const FULL: &str = "onnx-community/BiRefNet-ONNX";
const FULL_REV: &str = "534d3c82d3bb8b2f0867db6dfbc3a525b8e42f67";
const LITE: &str = "onnx-community/BiRefNet_lite-ONNX";
const LITE_REV: &str = "de15b22ba131738a16dff04aab8bdf8dc32e3ac1";

pub const MODELS: [Model; 4] = [
    Model {
        quality: Quality::Best,
        precision: Precision::Fp16,
        repo: FULL,
        revision: FULL_REV,
        file: "onnx/model_fp16.onnx",
        size: 489_666_272,
        sha256: "3654c741eb80bd926ada8fed1713b506ccf8d30eb1f6487e87eb9f234f33df09",
    },
    Model {
        quality: Quality::Best,
        precision: Precision::Fp32,
        repo: FULL,
        revision: FULL_REV,
        file: "onnx/model.onnx",
        size: 972_666_916,
        sha256: "58f621f00f5d756097615970a88a791584600dcf7c45b18a0a6267535a1ebd3c",
    },
    Model {
        quality: Quality::Fast,
        precision: Precision::Fp16,
        repo: LITE,
        revision: LITE_REV,
        file: "onnx/model_fp16.onnx",
        size: 114_538_221,
        sha256: "d39b897ceb16ae654c1731f3dba0cf9b368d9cae74b5a57459b455cc8bfec402",
    },
    Model {
        quality: Quality::Fast,
        precision: Precision::Fp32,
        repo: LITE,
        revision: LITE_REV,
        file: "onnx/model.onnx",
        size: 224_005_088,
        sha256: "5600024376f572a557870a5eb0afb1e5961636bef4e1e22132025467d0f03333",
    },
];

impl Model {
    pub fn find(quality: Quality, precision: Precision) -> Model {
        MODELS
            .into_iter()
            .find(|m| m.quality == quality && m.precision == precision)
            .expect("every pair is listed")
    }

    /// "BiRefNet" or "BiRefNet lite".
    pub fn name(&self) -> &'static str {
        match self.quality {
            Quality::Best => "BiRefNet",
            Quality::Fast => "BiRefNet lite",
        }
    }

    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.file
        )
    }

    /// Where the model's repository can be read about (shown before downloading).
    pub fn page(&self) -> String {
        format!("https://huggingface.co/{}", self.repo)
    }

    /// File name in the models folder: names the revision, so a new pin is a new file.
    pub fn file_name(&self) -> String {
        let name = match self.quality {
            Quality::Best => "birefnet",
            Quality::Fast => "birefnet-lite",
        };
        let precision = match self.precision {
            Precision::Fp16 => "fp16",
            Precision::Fp32 => "fp32",
        };
        format!("{name}-{precision}-{}.onnx", &self.revision[..7])
    }

    pub fn path_in(&self, dir: &Path) -> PathBuf {
        dir.join(self.file_name())
    }

    /// Downloaded (it was checked when it arrived; a file of the wrong size is not counted).
    pub fn is_present_in(&self, dir: &Path) -> bool {
        std::fs::metadata(self.path_in(dir)).is_ok_and(|m| m.len() == self.size)
    }
}

/// `$PNN_MODELS_DIR`, else `$XDG_DATA_HOME/paintnotnet/models`, else
/// `~/.local/share/paintnotnet/models`.
pub fn models_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("PNN_MODELS_DIR") {
        return PathBuf::from(dir);
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_else(|| PathBuf::from("."));
    data.join("paintnotnet/models")
}

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("download failed: {0}")]
    Http(String),
    #[error("couldn't write the model: {0}")]
    Io(#[from] std::io::Error),
    #[error("the download was {got} bytes, expected {expected}")]
    Size { got: u64, expected: u64 },
    #[error("the download's SHA-256 didn't match (got {got})")]
    Checksum { got: String },
    #[error("cancelled")]
    Cancelled,
}

/// Progress of a download, readable from another thread.
#[derive(Debug, Default)]
pub struct Progress {
    pub received: AtomicU64,
    pub cancel: AtomicBool,
}

/// Download `model` into `dir`: streamed to a `.part` file while hashing, checked against
/// the listed size and SHA-256, then renamed into place.
pub fn download(model: &Model, dir: &Path, progress: &Progress) -> Result<PathBuf, DownloadError> {
    download_from(
        &model.url(),
        model.size,
        model.sha256,
        &model.path_in(dir),
        progress,
    )
}

pub(crate) fn download_from(
    url: &str,
    size: u64,
    sha256: &str,
    dest: &Path,
    progress: &Progress,
) -> Result<PathBuf, DownloadError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part = dest.with_extension("onnx.part");
    let result = (|| {
        let response = ureq::get(url)
            .call()
            .map_err(|e| DownloadError::Http(e.to_string()))?;
        let mut body = response.into_body().into_reader();
        let mut file = std::fs::File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 20];
        let mut got = 0u64;
        loop {
            if progress.cancel.load(Ordering::Relaxed) {
                return Err(DownloadError::Cancelled);
            }
            let n = body
                .read(&mut buf)
                .map_err(|e| DownloadError::Http(e.to_string()))?;
            if n == 0 {
                break;
            }
            got += n as u64;
            if got > size {
                return Err(DownloadError::Size {
                    got,
                    expected: size,
                });
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n])?;
            progress.received.store(got, Ordering::Relaxed);
        }
        if got != size {
            return Err(DownloadError::Size {
                got,
                expected: size,
            });
        }
        let digest: String = hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if digest != sha256 {
            return Err(DownloadError::Checksum { got: digest });
        }
        file.sync_all()?;
        drop(file);
        std::fs::rename(&part, dest)?;
        Ok(dest.to_owned())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;

    /// Serve `body` once on localhost; returns the URL.
    fn serve(body: Vec<u8>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    line.clear();
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}/model.onnx")
    }

    fn sha(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    }

    #[test]
    fn a_good_download_lands_and_a_bad_one_leaves_nothing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body: Vec<u8> = (0..3_000_000u32).map(|i| (i * 7) as u8).collect();
        let dest = dir.path().join("m.onnx");
        let progress = Progress::default();
        let got = download_from(
            &serve(body.clone()),
            body.len() as u64,
            &sha(&body),
            &dest,
            &progress,
        )
        .expect("download");
        assert_eq!(std::fs::read(&got).expect("read"), body);
        assert_eq!(progress.received.load(Ordering::Relaxed), body.len() as u64);

        // Wrong checksum: refused, and neither the file nor a .part is left.
        let dest2 = dir.path().join("bad.onnx");
        let wrong = "0".repeat(64);
        let e = download_from(
            &serve(body.clone()),
            body.len() as u64,
            &wrong,
            &dest2,
            &Progress::default(),
        )
        .expect_err("checksum");
        assert!(matches!(e, DownloadError::Checksum { .. }), "{e}");
        assert!(!dest2.exists());
        assert!(!dest2.with_extension("onnx.part").exists());

        // Wrong size.
        let e = download_from(
            &serve(body.clone()),
            10,
            &sha(&body),
            &dest2,
            &Progress::default(),
        )
        .expect_err("size");
        assert!(matches!(e, DownloadError::Size { .. }), "{e}");

        // Cancelled before it starts reading.
        let cancelled = Progress::default();
        cancelled.cancel.store(true, Ordering::Relaxed);
        let e = download_from(
            &serve(body.clone()),
            body.len() as u64,
            &sha(&body),
            &dest2,
            &cancelled,
        )
        .expect_err("cancel");
        assert!(matches!(e, DownloadError::Cancelled), "{e}");
        assert!(!dest2.with_extension("onnx.part").exists());
    }

    #[test]
    fn file_names_and_presence() {
        let m = Model::find(Quality::Fast, Precision::Fp16);
        assert_eq!(m.file_name(), "birefnet-lite-fp16-de15b22.onnx");
        assert!(
            m.url().ends_with(
                "/resolve/de15b22ba131738a16dff04aab8bdf8dc32e3ac1/onnx/model_fp16.onnx"
            )
        );
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(!m.is_present_in(dir.path()));
        std::fs::write(m.path_in(dir.path()), b"short").expect("write");
        assert!(
            !m.is_present_in(dir.path()),
            "a file of the wrong size isn't the model"
        );
    }
}
