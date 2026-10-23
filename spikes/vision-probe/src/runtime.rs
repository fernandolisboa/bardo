//! ONNX Runtime sessions: which execution provider, and plain-slice
//! inputs and outputs.

use std::path::Path;

use anyhow::{Context, Result};
use ort::session::Session;
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::value::TensorRef;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Ep {
    Cpu,
    DirectMl,
}

impl Ep {
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "cpu" => Ok(Ep::Cpu),
            "directml" | "dml" => Ok(Ep::DirectMl),
            _ => anyhow::bail!("unknown execution provider {s} (cpu or directml)"),
        }
    }
}

pub struct Model {
    pub name: String,
    pub session: Session,
    /// What the session actually runs on: DirectML falls back to the CPU
    /// when it cannot start.
    pub ep: Ep,
    pub fallback_reason: Option<String>,
}

pub fn load(path: &Path, wanted: Ep, threads: usize) -> Result<Model> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("model")
        .to_owned();
    let (session, ep, fallback_reason) = match wanted {
        Ep::Cpu => (cpu(path, threads)?, Ep::Cpu, None),
        Ep::DirectMl => match directml(path) {
            Ok(session) => (session, Ep::DirectMl, None),
            Err(e) => (cpu(path, threads)?, Ep::Cpu, Some(format!("{e:#}"))),
        },
    };
    Ok(Model {
        name,
        session,
        ep,
        fallback_reason,
    })
}

/// Builder errors carry the builder back, which is not `Send`; keep the
/// message.
fn b<T>(r: std::result::Result<T, ort::Error<SessionBuilder>>) -> Result<T> {
    r.map_err(|e| anyhow::anyhow!("{e}"))
}

fn cpu(path: &Path, threads: usize) -> Result<Session> {
    let mut builder =
        b(Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3))?;
    if threads > 0 {
        builder = b(builder.with_intra_threads(threads))?;
    }
    builder
        .commit_from_file(path)
        .with_context(|| format!("loading {}", path.display()))
}

#[cfg(windows)]
fn directml(path: &Path) -> Result<Session> {
    use ort::execution_providers::DirectMLExecutionProvider;
    // DirectML needs memory patterns off and sequential execution.
    let builder = b(Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3))?;
    let builder = b(builder.with_memory_pattern(false))?;
    let builder = b(builder.with_parallel_execution(false))?;
    b(
        builder.with_execution_providers([DirectMLExecutionProvider::default()
            .build()
            .error_on_failure()]),
    )?
    .commit_from_file(path)
    .with_context(|| format!("loading {} on DirectML", path.display()))
}

#[cfg(not(windows))]
fn directml(_path: &Path) -> Result<Session> {
    anyhow::bail!("DirectML is only built on Windows")
}

impl Model {
    /// Runs the model on one f32 input and returns every output as
    /// (shape, values), in the model's output order.
    pub fn run(&mut self, shape: &[usize], data: &[f32]) -> Result<Vec<(Vec<usize>, Vec<f32>)>> {
        let input = TensorRef::from_array_view((shape.to_vec(), data))?;
        let outputs = self.session.run(ort::inputs![input])?;
        let mut all = Vec::with_capacity(outputs.len());
        for (_, value) in outputs.iter() {
            let (shape, values) = value.try_extract_tensor::<f32>()?;
            all.push((shape.iter().map(|&d| d as usize).collect(), values.to_vec()));
        }
        Ok(all)
    }

    pub fn describe(&self) -> String {
        let io = |v: Vec<String>| v.join(", ");
        format!(
            "{} on {:?}{}: in [{}] out [{}]",
            self.name,
            self.ep,
            self.fallback_reason
                .as_ref()
                .map(|r| format!(" (DirectML failed: {r})"))
                .unwrap_or_default(),
            io(self
                .session
                .inputs()
                .iter()
                .map(|i| format!("{} {:?}", i.name(), i.dtype()))
                .collect()),
            io(self
                .session
                .outputs()
                .iter()
                .map(|o| format!("{} {:?}", o.name(), o.dtype()))
                .collect()),
        )
    }
}
