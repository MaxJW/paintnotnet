//! Local AI for Remove Background and Select Subject: BiRefNet run with ONNX Runtime.
//!
//! Nothing here runs or downloads anything until asked. ONNX Runtime is the system's, loaded
//! the first time a model is needed; models are fetched only when the user agrees, checked
//! by SHA-256 and kept in [`models_dir`].

pub mod models;
pub mod prep;
pub mod refine;
pub mod runtime;

pub use models::{
    DownloadError, MODELS, Model, Precision, Progress, Quality, download, models_dir,
};
pub use refine::Refine;
pub use runtime::{AiError, Device, Runtime, Segmenter, runtime};
