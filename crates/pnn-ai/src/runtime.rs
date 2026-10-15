//! ONNX Runtime, loaded from the system when first needed (`libonnxruntime.so.1`, or
//! `$ORT_DYLIB_PATH`), and a loaded model.
//!
//! The GPU (CUDA) is used when the runtime was built with it and an NVIDIA driver is loaded;
//! if CUDA fails to start for a model, it runs on the CPU instead. Models are run in a helper
//! process (`worker`), which is what makes CUDA start with Arch's package.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ort::ep::{self, ExecutionProvider};
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::{Tensor, TensorElementType, ValueType};

use crate::models::{Model, Precision};
use crate::prep;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Device {
    Cuda,
    Cpu,
}

impl Device {
    pub fn label(self) -> &'static str {
        match self {
            Self::Cuda => "GPU (CUDA)",
            Self::Cpu => "CPU",
        }
    }

    /// The model precision that runs best here.
    pub fn precision(self) -> Precision {
        match self {
            Self::Cuda => Precision::Fp16,
            Self::Cpu => Precision::Fp32,
        }
    }
}

pub struct Runtime {
    /// Where models will run (CUDA if it looks usable).
    pub device: Device,
    env: ort::environment::Environment,
}

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error(
        "ONNX Runtime isn't installed ({0}). Install the onnxruntime package \
         (onnxruntime-opt-cuda to use an NVIDIA GPU)."
    )]
    NoRuntime(String),
    #[error("couldn't load the model: {0}")]
    Load(String),
    #[error("the model failed: {0}")]
    Run(String),
}

static RUNTIME: OnceLock<Result<Runtime, String>> = OnceLock::new();

fn library() -> PathBuf {
    std::env::var_os("ORT_DYLIB_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("libonnxruntime.so.1"))
}

/// Load ONNX Runtime (once per process).
pub fn runtime() -> Result<&'static Runtime, AiError> {
    RUNTIME
        .get_or_init(|| {
            let lib = library();
            let builder = ort::init_from(&lib).map_err(|e| e.to_string())?;
            let env = builder
                .with_name("paintnotnet")
                .build()
                .map_err(|e| e.to_string())?;
            let cuda = ep::CUDA::is_available().unwrap_or(false)
                && nvidia_driver()
                && std::env::var_os("PNN_AI_CPU").is_none();
            log::info!("ONNX Runtime from {}; CUDA: {cuda}", lib.display());
            Ok(Runtime {
                device: if cuda { Device::Cuda } else { Device::Cpu },
                env,
            })
        })
        .as_ref()
        .map_err(|e| AiError::NoRuntime(e.clone()))
}

/// The NVIDIA kernel driver is loaded (CUDA can't work without it).
fn nvidia_driver() -> bool {
    Path::new("/proc/driver/nvidia/version").exists()
}

/// A model loaded and ready to segment images.
pub struct Segmenter {
    pub model: Model,
    pub device: Device,
    session: Session,
    input: String,
    output: String,
    half_input: bool,
}

impl Segmenter {
    /// Load `model` from `path`, on the GPU if the runtime has one, else the CPU.
    pub fn load(model: Model, path: &Path) -> Result<Segmenter, AiError> {
        let rt = runtime()?;
        let build = |device: Device| -> ort::Result<Session> {
            let mut b = Session::builder(&rt.env)?
                .with_optimization_level(GraphOptimizationLevel::Level3)?;
            if device == Device::Cuda {
                b = b.with_execution_providers([ep::CUDA::default().build().error_on_failure()])?;
            }
            b.commit_from_file(path)
        };
        let (session, device) = match rt.device {
            Device::Cuda => match build(Device::Cuda) {
                Ok(s) => (s, Device::Cuda),
                Err(e) => {
                    log::warn!(
                        "CUDA didn't start for {} ({e}); using the CPU",
                        model.name()
                    );
                    (
                        build(Device::Cpu).map_err(|e| AiError::Load(e.to_string()))?,
                        Device::Cpu,
                    )
                }
            },
            Device::Cpu => (
                build(Device::Cpu).map_err(|e| AiError::Load(e.to_string()))?,
                Device::Cpu,
            ),
        };
        let first = |outlets: &[ort::value::Outlet]| {
            outlets
                .first()
                .map(|o| (o.name().to_owned(), o.dtype().clone()))
                .ok_or_else(|| AiError::Load("the model has no inputs or outputs".into()))
        };
        let (input, input_type) = first(session.inputs())?;
        let (output, _) = first(session.outputs())?;
        let half_input = matches!(
            input_type,
            ValueType::Tensor {
                ty: TensorElementType::Float16,
                ..
            }
        );
        Ok(Segmenter {
            model,
            device,
            session,
            input,
            output,
            half_input,
        })
    }

    /// Coverage of the image's main subject, 0–255 per pixel, for `rgb` (interleaved,
    /// `w × h`, straight colour).
    pub fn matte(&mut self, rgb: &[u8], w: usize, h: usize) -> Result<Vec<u8>, AiError> {
        assert_eq!(rgb.len(), w * h * 3);
        let data = prep::input(rgb, w, h);
        let shape = [1usize, 3, prep::SIZE, prep::SIZE];
        let run = |e: ort::Error| AiError::Run(e.to_string());
        let outputs = if self.half_input {
            let data: Vec<half::f16> = data.into_iter().map(half::f16::from_f32).collect();
            let t = Tensor::from_array((shape, data)).map_err(run)?;
            self.session
                .run(ort::inputs![self.input.as_str() => t])
                .map_err(run)?
        } else {
            let t = Tensor::from_array((shape, data)).map_err(run)?;
            self.session
                .run(ort::inputs![self.input.as_str() => t])
                .map_err(run)?
        };
        let value = &outputs[self.output.as_str()];
        let logits: Vec<f32> = match value.try_extract_tensor::<f32>() {
            Ok((_, v)) => v.to_vec(),
            Err(_) => {
                let (_, v) = value.try_extract_tensor::<half::f16>().map_err(run)?;
                v.iter().map(|x| x.to_f32()).collect()
            }
        };
        if logits.len() != prep::SIZE * prep::SIZE {
            return Err(AiError::Run(format!(
                "expected a {0}×{0} mask, got {1} values",
                prep::SIZE,
                logits.len()
            )));
        }
        Ok(prep::output(&logits, w, h))
    }
}
