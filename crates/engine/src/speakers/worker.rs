//! Supervises the `tuldmi-speakers` worker process.
//!
//! sherpa-onnx can end its process from deep inside C++ (`_Exit`), so it never runs in the app:
//! the worker is a child process started on first use. Any failure (spawn error, crash, timeout,
//! malformed or invalid reply) kills it; the next request starts a fresh one. After
//! `MAX_FAILURES` consecutive failures the worker stays off for the rest of the session.
//! The worker's stderr is discarded; it is never given or shown transcript text.

use super::protocol::{self, Reply};
use crate::{Error, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::io;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::time::Duration;

pub const MAX_FAILURES: u32 = 3;

/// Where the worker and its two models are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub program: PathBuf,
    pub segmentation: PathBuf,
    pub embedding: PathBuf,
}

struct Running {
    child: Child,
    stdin: ChildStdin,
    replies: Receiver<io::Result<Reply>>,
}

pub struct Worker {
    config: WorkerConfig,
    running: Option<Running>,
    failures: u32,
    /// Overall time limit per request: this, plus `TIMEOUT_PER_AUDIO` times the audio length.
    base_timeout: Duration,
}

const TIMEOUT_PER_AUDIO: u32 = 2;

impl Worker {
    pub fn new(config: WorkerConfig) -> Self {
        Worker { config, running: None, failures: 0, base_timeout: Duration::from_secs(30) }
    }

    pub fn config(&self) -> &WorkerConfig {
        &self.config
    }

    /// True once the worker failed `MAX_FAILURES` times in a row.
    pub fn given_up(&self) -> bool {
        self.failures >= MAX_FAILURES
    }

    /// Separate the speakers in `samples` (16 kHz mono). The reply is validated.
    pub fn separate(&mut self, samples: &[f32]) -> Result<Reply> {
        if self.given_up() {
            return Err(Error::Other("speaker separation is unavailable".into()));
        }
        let duration_ms = samples.len() as i64 / 16;
        let result = self.request(samples).and_then(|reply| {
            protocol::validate(&reply, duration_ms).map_err(|e| Error::Other(e.into()))?;
            Ok(reply)
        });
        match &result {
            Ok(_) => self.failures = 0,
            Err(_) => {
                self.stop();
                self.failures += 1;
            }
        }
        result
    }

    fn request(&mut self, samples: &[f32]) -> Result<Reply> {
        if self.running.is_none() {
            self.running = Some(self.spawn()?);
        }
        let running = self.running.as_mut().ok_or_else(|| Error::Other("worker missing".into()))?;
        protocol::write_request(&mut running.stdin, samples)
            .map_err(|_| Error::Other("speaker worker stopped".into()))?;
        let audio = Duration::from_millis(samples.len() as u64 / 16);
        match running.replies.recv_timeout(self.base_timeout + audio * TIMEOUT_PER_AUDIO) {
            Ok(Ok(reply)) => Ok(reply),
            Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => {
                Err(Error::Other("speaker worker stopped".into()))
            }
            Err(RecvTimeoutError::Timeout) => Err(Error::Other("speaker worker timed out".into())),
        }
    }

    fn spawn(&self) -> Result<Running> {
        let mut command = Command::new(&self.config.program);
        command
            .arg("--segmentation")
            .arg(&self.config.segmentation)
            .arg("--embedding")
            .arg(&self.config.embedding)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
            command.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
        }
        let mut child = command
            .spawn()
            .map_err(|_| Error::Other("could not start the speaker worker".into()))?;
        let (stdin, stdout) = match (child.stdin.take(), child.stdout.take()) {
            (Some(stdin), Some(stdout)) => (stdin, stdout),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Error::Other("speaker worker has no pipes".into()));
            }
        };
        let (send, replies) = crossbeam_channel::bounded(1);
        let reader = std::thread::Builder::new()
            .name("tuldmi-speakers-reader".into())
            .spawn(move || {
                let mut stdout = io::BufReader::new(stdout);
                loop {
                    let reply = protocol::read_reply(&mut stdout);
                    let failed = reply.is_err();
                    if send.send(reply).is_err() || failed {
                        return;
                    }
                }
            });
        if reader.is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error::Other("could not start the speaker worker reader".into()));
        }
        Ok(Running { child, stdin, replies })
    }

    /// Kill the worker (if running). Closing stdin alone would let it finish a long request.
    pub fn stop(&mut self) {
        if let Some(mut running) = self.running.take() {
            drop(running.stdin);
            let _ = running.child.kill();
            let _ = running.child.wait();
        }
    }

    #[cfg(test)]
    fn with_timeout(mut self, base: Duration) -> Self {
        self.base_timeout = base;
        self
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(program: &str) -> WorkerConfig {
        WorkerConfig { program: program.into(), segmentation: "seg.onnx".into(), embedding: "emb.onnx".into() }
    }

    #[test]
    fn a_missing_worker_fails_and_gives_up_after_repeated_failures() {
        let mut worker = Worker::new(config("tuldmi-speakers-that-does-not-exist"));
        for _ in 0..MAX_FAILURES {
            assert!(worker.separate(&[0.0; 160]).is_err());
        }
        assert!(worker.given_up());
        assert!(worker.separate(&[0.0; 160]).is_err());
    }

    /// A worker that never answers is killed at the timeout instead of hanging transcription.
    #[test]
    fn a_silent_worker_times_out() {
        // A process that reads stdin forever and never writes a reply.
        #[cfg(windows)]
        let program = "C:\\Windows\\System32\\findstr.exe";
        #[cfg(not(windows))]
        let program = "/bin/cat";
        if !std::path::Path::new(program).exists() {
            return;
        }
        // `cat` echoes the request back, which is not a valid reply either: both must fail fast.
        let mut worker = Worker::new(config(program)).with_timeout(Duration::from_millis(300));
        let started = std::time::Instant::now();
        assert!(worker.separate(&[0.0; 16]).is_err());
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(!worker.given_up());
    }
}
