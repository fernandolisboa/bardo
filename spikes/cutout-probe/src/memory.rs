//! The process's memory while something runs, sampled from another thread:
//! the largest resident size seen. Portable and without `unsafe`, at the
//! price of missing a peak shorter than the sampling step.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System};

pub struct Sampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    thread: Option<JoinHandle<()>>,
}

fn resident(system: &mut System, pid: Pid) -> u64 {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        ProcessRefreshKind::new().with_memory(),
    );
    system.process(pid).map(|p| p.memory()).unwrap_or(0)
}

impl Sampler {
    pub fn start() -> Sampler {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let (stop2, peak2) = (stop.clone(), peak.clone());
        let thread = std::thread::spawn(move || {
            let mut system = System::new();
            let pid = Pid::from_u32(std::process::id());
            while !stop2.load(Ordering::Relaxed) {
                peak2.fetch_max(resident(&mut system, pid), Ordering::Relaxed);
                std::thread::sleep(Duration::from_millis(10));
            }
            peak2.fetch_max(resident(&mut system, pid), Ordering::Relaxed);
        });
        Sampler {
            stop,
            peak,
            thread: Some(thread),
        }
    }

    /// Stops sampling and returns the peak in megabytes.
    pub fn finish(mut self) -> f64 {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        self.peak.load(Ordering::Relaxed) as f64 / (1024.0 * 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sees_the_process_and_an_allocation() {
        let before = Sampler::start().finish();
        assert!(before > 0.0, "the process has no resident memory");
        let sampler = Sampler::start();
        let block = std::hint::black_box(vec![1u8; 64 * 1024 * 1024]);
        std::thread::sleep(Duration::from_millis(50));
        let peak = sampler.finish();
        assert!(
            peak > before + 48.0,
            "peak {peak} MB from {before} MB with {} bytes held",
            block.len()
        );
    }
}
