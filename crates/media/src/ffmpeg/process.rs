//! Running an ffmpeg child to the end: progress from `-progress pipe:1`,
//! cancel by killing it, and the end of stderr kept for the error message.

use std::io::{BufRead as _, BufReader, Read as _};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use super::{MediaError, Monitor, hidden};

/// How much of stderr is kept: enough for ffmpeg's last error lines and for
/// the loudnorm JSON summary, bounded so a chatty run cannot grow it.
const LOG_TAIL: usize = 64 * 1024;

/// How often a run checks [`Monitor::should_stop`] while ffmpeg is quiet.
const POLL: Duration = Duration::from_millis(100);

/// Which part of a job's progress a run covers: its output reaching
/// `total` moves the job from `from` to `to` (fractions of the job).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Span {
    pub total: Duration,
    pub from: f32,
    pub to: f32,
}

impl Span {
    pub fn whole(total: Duration) -> Span {
        Span::part(total, 0.0, 1.0)
    }

    pub fn part(total: Duration, from: f32, to: f32) -> Span {
        Span { total, from, to }
    }

    fn fraction(&self, done: Duration) -> f32 {
        let ratio = if self.total.is_zero() {
            1.0
        } else {
            (done.as_secs_f64() / self.total.as_secs_f64()).clamp(0.0, 1.0) as f32
        };
        self.from + (self.to - self.from) * ratio
    }
}

/// Runs `command` (an ffmpeg invocation without progress options) to the
/// end, reporting progress over `span` and honouring cancel. Returns what
/// ffmpeg printed on stderr (its tail).
pub(super) fn run(
    command: Command,
    monitor: &dyn Monitor,
    span: Span,
) -> Result<String, MediaError> {
    // Global options go first, ahead of any input or output.
    let mut command = {
        let mut with_progress = hidden(Command::new(command.get_program()));
        with_progress
            .args(["-progress", "pipe:1", "-stats_period", "0.25", "-nostats"])
            .args(command.get_args())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        with_progress
    };
    let program = program_name(&command);
    let mut child = command.spawn().map_err(|source| MediaError::Spawn {
        program: program.clone(),
        source,
    })?;

    let stderr = collect_stderr(&mut child);
    let (sender, progress) = mpsc::channel();
    let stdout = child.stdout.take().expect("stdout is piped");
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if let Some(done) = progress_time(&line)
                && sender.send(done).is_err()
            {
                break;
            }
        }
    });

    loop {
        if monitor.should_stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(MediaError::Cancelled);
        }
        match progress.recv_timeout(POLL) {
            Ok(done) => monitor.progress(span.fraction(done)),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait()?;
    let log = stderr.join().unwrap_or_default();
    if status.success() {
        Ok(log)
    } else {
        Err(MediaError::Failed {
            program,
            status: status.to_string(),
            log: last_lines(&log, 5),
        })
    }
}

/// Runs a short command and returns its stdout, failing with the end of
/// stderr if it exits with an error.
pub(super) fn output(mut command: Command) -> Result<Vec<u8>, MediaError> {
    let program = program_name(&command);
    let output = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| MediaError::Spawn {
            program: program.clone(),
            source,
        })?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(MediaError::Failed {
            program,
            status: output.status.to_string(),
            log: last_lines(&String::from_utf8_lossy(&output.stderr), 5),
        })
    }
}

/// Reads the child's stderr on a thread, keeping its last [`LOG_TAIL`]
/// bytes, so a full pipe never stalls ffmpeg.
pub(super) fn collect_stderr(child: &mut Child) -> thread::JoinHandle<String> {
    let mut stderr = child.stderr.take().expect("stderr is piped");
    thread::spawn(move || {
        let mut tail = Vec::new();
        let mut buffer = [0; 8192];
        while let Ok(read) = stderr.read(&mut buffer) {
            if read == 0 {
                break;
            }
            tail.extend_from_slice(&buffer[..read]);
            if tail.len() > 2 * LOG_TAIL {
                tail.drain(..tail.len() - LOG_TAIL);
            }
        }
        String::from_utf8_lossy(&tail).into_owned()
    })
}

/// A child process killed when this is dropped: a stream's producer.
pub(super) struct ChildGuard(pub Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts `command`, which writes interleaved f32 samples to stdout, and
/// streams them in chunks of about 50 ms, two seconds ahead at most.
pub(super) fn spawn_pcm(
    mut command: Command,
    channels: u16,
    sample_rate: u32,
) -> Result<crate::PcmStream, MediaError> {
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let program = program_name(&command);
    let mut child = command
        .spawn()
        .map_err(|source| MediaError::Spawn { program, source })?;
    // Nobody reads the log of a stream; draining it keeps ffmpeg from
    // stalling on a full pipe.
    let _ = collect_stderr(&mut child);
    let mut stdout = child.stdout.take().expect("stdout is piped");
    let (stream, sender) =
        crate::PcmStream::channel(channels, sample_rate, 40, Some(Box::new(ChildGuard(child))));
    let frame_bytes = usize::from(channels.max(1)) * 4;
    let chunk_bytes = (sample_rate as usize / 20).max(1) * frame_bytes;
    thread::spawn(move || {
        let mut leftover: Vec<u8> = Vec::new();
        let mut buffer = vec![0u8; chunk_bytes];
        loop {
            let read = match stdout.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            leftover.extend_from_slice(&buffer[..read]);
            if leftover.len() < chunk_bytes {
                continue;
            }
            let whole = leftover.len() - leftover.len() % frame_bytes;
            let samples = samples_of(&leftover[..whole]);
            leftover.drain(..whole);
            if !sender.send(samples) {
                return;
            }
        }
        let whole = leftover.len() - leftover.len() % frame_bytes;
        if whole > 0 {
            sender.send(samples_of(&leftover[..whole]));
        }
    });
    Ok(stream)
}

/// Little-endian f32 samples.
fn samples_of(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|sample| f32::from_le_bytes(*sample))
        .collect()
}

pub(super) fn program_name(command: &Command) -> String {
    std::path::Path::new(command.get_program())
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "ffmpeg".into())
}

/// The output time in a `-progress` line ("out_time_us=1234567").
/// ffmpeg prints "N/A" before the first frame.
fn progress_time(line: &str) -> Option<Duration> {
    let micros = line
        .strip_prefix("out_time_us=")?
        .trim()
        .parse::<u64>()
        .ok()?;
    Some(Duration::from_micros(micros))
}

fn last_lines(log: &str, count: usize) -> String {
    let lines: Vec<&str> = log.lines().filter(|line| !line.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(count)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_lines_give_output_time() {
        assert_eq!(
            progress_time("out_time_us=1500000"),
            Some(Duration::from_millis(1500))
        );
        assert_eq!(progress_time("out_time_us=N/A"), None);
        assert_eq!(progress_time("out_time=00:00:01.500000"), None);
        assert_eq!(progress_time("frame=12"), None);
    }

    #[test]
    fn spans_map_output_time_onto_the_job() {
        let span = Span::part(Duration::from_secs(10), 0.1, 1.0);
        assert_eq!(span.fraction(Duration::ZERO), 0.1);
        assert!((span.fraction(Duration::from_secs(5)) - 0.55).abs() < 1e-6);
        assert_eq!(span.fraction(Duration::from_secs(20)), 1.0);
        assert_eq!(Span::whole(Duration::ZERO).fraction(Duration::ZERO), 1.0);
    }

    #[test]
    fn error_message_keeps_the_last_lines() {
        let log = "a\n\nb\nc\nd\ne\nf\n";
        assert_eq!(last_lines(log, 3), "d\ne\nf");
        assert_eq!(last_lines("only", 5), "only");
    }
}
