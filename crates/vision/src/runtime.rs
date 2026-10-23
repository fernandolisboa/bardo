//! ONNX Runtime sessions (ADR-0024): DirectML when it starts, the CPU
//! otherwise, with plain-slice inputs and outputs.

use std::path::{Path, PathBuf};

use bardo_domain::{Device, VisionError};
use ort::session::Session;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::value::TensorRef;

/// Which device a session is asked to run on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wanted {
    /// DirectML where it starts (Windows), else the CPU.
    Gpu,
    Cpu,
}

/// One model's session, and where it runs.
pub(crate) struct Model {
    path: PathBuf,
    /// CPU threads; 0 for ONNX Runtime's default (every core).
    threads: usize,
    session: Session,
    device: Device,
}

fn failed(error: impl std::fmt::Display) -> VisionError {
    VisionError::Failed(error.to_string())
}

/// Builder errors carry the builder back, which is not `Send`; keep the
/// message.
fn b<T>(result: Result<T, ort::Error<SessionBuilder>>) -> Result<T, VisionError> {
    result.map_err(failed)
}

fn cpu(path: &Path, threads: usize) -> Result<Session, VisionError> {
    let mut builder = b(Session::builder()
        .map_err(failed)?
        .with_optimization_level(GraphOptimizationLevel::Level3))?;
    if threads > 0 {
        builder = b(builder.with_intra_threads(threads))?;
    }
    builder
        .commit_from_file(path)
        .map_err(|e| failed(format!("loading {}: {e}", path.display())))
}

#[cfg(windows)]
fn directml(path: &Path) -> Result<Session, VisionError> {
    use ort::execution_providers::DirectMLExecutionProvider;
    // DirectML needs memory patterns off and sequential execution.
    let builder = b(Session::builder()
        .map_err(failed)?
        .with_optimization_level(GraphOptimizationLevel::Level3))?;
    let builder = b(builder.with_memory_pattern(false))?;
    let builder = b(builder.with_parallel_execution(false))?;
    b(
        builder.with_execution_providers([DirectMLExecutionProvider::default()
            .build()
            .error_on_failure()]),
    )?
    .commit_from_file(path)
    .map_err(|e| failed(format!("loading {} on DirectML: {e}", path.display())))
}

#[cfg(not(windows))]
fn directml(_path: &Path) -> Result<Session, VisionError> {
    Err(VisionError::Failed(
        "DirectML runs only on Windows".to_owned(),
    ))
}

impl Model {
    pub(crate) fn load(path: &Path, wanted: Wanted, threads: usize) -> Result<Model, VisionError> {
        let (session, device) = match wanted {
            Wanted::Cpu => (cpu(path, threads)?, Device::Cpu { fallback: None }),
            Wanted::Gpu => match directml(path) {
                Ok(session) => (session, Device::Gpu),
                Err(error) => {
                    let reason = error.to_string();
                    tracing::info!(model = %path.display(), %reason, "running on the CPU");
                    (
                        cpu(path, threads)?,
                        Device::Cpu {
                            fallback: Some(reason),
                        },
                    )
                }
            },
        };
        tracing::info!(model = %path.display(), ?device, "vision model loaded");
        Ok(Model {
            path: path.to_owned(),
            threads,
            session,
            device,
        })
    }

    pub(crate) fn device(&self) -> &Device {
        &self.device
    }

    /// Runs the model on one f32 input and returns every output's values,
    /// in the model's output order. A run that fails on the graphics card
    /// (out of its memory, a driver fault) is run again on the CPU, which
    /// the model then keeps.
    pub(crate) fn run(
        &mut self,
        shape: &[usize],
        data: &[f32],
    ) -> Result<Vec<Vec<f32>>, VisionError> {
        match self.try_run(shape, data) {
            Ok(outputs) => Ok(outputs),
            Err(error) if self.device == Device::Gpu => {
                let reason = error.to_string();
                tracing::warn!(model = %self.path.display(), %reason, "moving to the CPU");
                self.session = cpu(&self.path, self.threads)?;
                self.device = Device::Cpu {
                    fallback: Some(reason),
                };
                self.try_run(shape, data)
            }
            Err(error) => Err(error),
        }
    }

    fn try_run(&mut self, shape: &[usize], data: &[f32]) -> Result<Vec<Vec<f32>>, VisionError> {
        let input = TensorRef::from_array_view((shape.to_vec(), data)).map_err(failed)?;
        let outputs = self.session.run(ort::inputs![input]).map_err(failed)?;
        let mut all = Vec::with_capacity(outputs.len());
        for (_, value) in outputs.iter() {
            let (_, values) = value.try_extract_tensor::<f32>().map_err(failed)?;
            all.push(values.to_vec());
        }
        Ok(all)
    }
}
