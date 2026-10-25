//! ONNX Runtime sessions as ADR-0024 runs them: DirectML when it starts,
//! the CPU otherwise. Optionally profiled, to tell which execution
//! provider ran each node: DirectML hands the operators it lacks to the
//! CPU without saying so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use ort::session::builder::{GraphOptimizationLevel, SessionBuilder};
use ort::session::{Session, SessionInputValue};
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
    session: Session,
    /// What the session actually runs on: DirectML falls back to the CPU
    /// when it cannot start.
    pub ep: Ep,
    pub fallback_reason: Option<String>,
}

/// Builder errors carry the builder back, which is not `Send`; keep the
/// message.
fn b<T>(r: std::result::Result<T, ort::Error<SessionBuilder>>) -> Result<T> {
    r.map_err(|e| anyhow::anyhow!("{e}"))
}

fn builder(profile: Option<&Path>) -> Result<SessionBuilder> {
    let builder = b(Session::builder()?.with_optimization_level(GraphOptimizationLevel::Level3))?;
    match profile {
        Some(path) => b(builder.with_profiling(path)),
        None => Ok(builder),
    }
}

fn cpu(path: &Path, threads: usize, profile: Option<&Path>) -> Result<Session> {
    let mut builder = builder(profile)?;
    if threads > 0 {
        builder = b(builder.with_intra_threads(threads))?;
    }
    builder
        .commit_from_file(path)
        .with_context(|| format!("loading {}", path.display()))
}

#[cfg(windows)]
fn directml(path: &Path, profile: Option<&Path>) -> Result<Session> {
    use ort::execution_providers::DirectMLExecutionProvider;
    // DirectML needs memory patterns off and sequential execution.
    let builder = b(builder(profile)?.with_memory_pattern(false))?;
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
fn directml(_path: &Path, _profile: Option<&Path>) -> Result<Session> {
    anyhow::bail!("DirectML is only built on Windows")
}

impl Model {
    /// Loads `path` on `wanted`; `profile` names the file prefix ONNX
    /// Runtime writes its profile to, when wanted.
    pub fn load(path: &Path, wanted: Ep, threads: usize, profile: Option<&Path>) -> Result<Model> {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("model")
            .to_owned();
        let (session, ep, fallback_reason) = match wanted {
            Ep::Cpu => (cpu(path, threads, profile)?, Ep::Cpu, None),
            Ep::DirectMl => match directml(path, profile) {
                Ok(session) => (session, Ep::DirectMl, None),
                Err(e) => (
                    cpu(path, threads, profile)?,
                    Ep::Cpu,
                    Some(format!("{e:#}")),
                ),
            },
        };
        Ok(Model {
            name,
            session,
            ep,
            fallback_reason,
        })
    }

    /// The shapes of the model's inputs, fixed in every file here.
    pub fn input_shapes(&self) -> Vec<Vec<usize>> {
        self.session
            .inputs()
            .iter()
            .map(|i| {
                i.dtype()
                    .tensor_shape()
                    .map(|s| s.iter().map(|&d| d.max(1) as usize).collect())
                    .unwrap_or_default()
            })
            .collect()
    }

    /// Runs the model on f32 inputs, in the model's input order, and
    /// returns every output's values in its output order.
    pub fn run(&mut self, inputs: &[(&[usize], &[f32])]) -> Result<Vec<Vec<f32>>> {
        let values = inputs
            .iter()
            .map(|(shape, data)| {
                TensorRef::from_array_view((shape.to_vec(), *data)).map(SessionInputValue::from)
            })
            .collect::<ort::Result<Vec<_>>>()?;
        let outputs = self.session.run(values.as_slice())?;
        let mut all = Vec::with_capacity(outputs.len());
        for (_, value) in outputs.iter() {
            let (_, values) = value.try_extract_tensor::<f32>()?;
            all.push(values.to_vec());
        }
        Ok(all)
    }

    /// Ends profiling and counts, from the profile, the nodes each
    /// execution provider ran, with the operators of those on the CPU.
    pub fn placement(&mut self) -> Result<Placement> {
        let file = PathBuf::from(self.session.end_profiling()?);
        let text = std::fs::read_to_string(&file)
            .with_context(|| format!("reading the profile {}", file.display()))?;
        let _ = std::fs::remove_file(&file);
        Placement::from_profile(&text)
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

/// Which execution provider ran the nodes of one run.
#[derive(Debug, Default, PartialEq, serde::Serialize)]
pub struct Placement {
    /// Nodes run, by execution provider.
    pub nodes: BTreeMap<String, usize>,
    /// The CPU's nodes by operator, when another provider ran some too:
    /// what the graphics card could not take.
    pub on_cpu: BTreeMap<String, usize>,
}

impl Placement {
    /// From ONNX Runtime's profile (Chrome trace JSON): every
    /// `*_kernel_time` event of category `Node` names its operator and
    /// provider. Only the first run is counted.
    pub fn from_profile(text: &str) -> Result<Placement> {
        let events: Vec<serde_json::Value> = serde_json::from_str(text)?;
        let mut seen = std::collections::HashSet::new();
        let mut placement = Placement::default();
        let mut cpu_ops: BTreeMap<String, usize> = BTreeMap::new();
        for e in &events {
            let name = e["name"].as_str().unwrap_or_default();
            if e["cat"] != "Node"
                || !name.ends_with("_kernel_time")
                || !seen.insert(name.to_owned())
            {
                continue;
            }
            let provider = e["args"]["provider"].as_str().unwrap_or("unknown");
            *placement.nodes.entry(provider.to_owned()).or_default() += 1;
            if provider == "CPUExecutionProvider" {
                let op = e["args"]["op_name"].as_str().unwrap_or("?");
                *cpu_ops.entry(op.to_owned()).or_default() += 1;
            }
        }
        if placement.nodes.len() > 1 {
            placement.on_cpu = cpu_ops;
        }
        Ok(placement)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_counts_nodes_by_provider_once_per_node() {
        let profile = r#"[
            {"cat":"Session","name":"model_run","args":{}},
            {"cat":"Node","name":"conv_1_kernel_time","args":{"op_name":"Conv","provider":"DmlExecutionProvider"}},
            {"cat":"Node","name":"gs_kernel_time","args":{"op_name":"GridSample","provider":"CPUExecutionProvider"}},
            {"cat":"Node","name":"gs_fence_before","args":{"op_name":"GridSample","provider":"CPUExecutionProvider"}},
            {"cat":"Node","name":"conv_1_kernel_time","args":{"op_name":"Conv","provider":"DmlExecutionProvider"}}
        ]"#;
        let p = Placement::from_profile(profile).unwrap();
        assert_eq!(p.nodes["DmlExecutionProvider"], 1);
        assert_eq!(p.nodes["CPUExecutionProvider"], 1);
        assert_eq!(p.on_cpu["GridSample"], 1);
    }

    #[test]
    fn all_on_one_provider_lists_no_operators() {
        let profile = r#"[{"cat":"Node","name":"a_kernel_time","args":{"op_name":"Conv","provider":"CPUExecutionProvider"}}]"#;
        let p = Placement::from_profile(profile).unwrap();
        assert!(p.on_cpu.is_empty());
    }
}
