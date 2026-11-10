//! Supervises the `tuldmi-speakers` worker process.
//!
//! sherpa-onnx can end its process from deep inside C++ (`_Exit`), so it never runs in the app:
//! the worker is a child process started on first use. Any failure (spawn error, crash, timeout,
//! malformed or invalid reply) kills it; the next request starts a fresh one. After
//! `MAX_FAILURES` consecutive failures the worker stays off for the rest of the session.
//! The worker's stderr is discarded; it is never given or shown transcript text.

use super::protocol::{self, Kind, Reply, Sound};
use crate::{Error, Result};
use crossbeam_channel::{Receiver, RecvTimeoutError};
use std::io;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const MAX_FAILURES: u32 = 3;

/// Where the worker and its two models are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerConfig {
    pub program: PathBuf,
    /// Speaker separation: (segmentation model, embedding model).
    pub speakers: Option<(PathBuf, PathBuf)>,
    /// Music rejection: (sound classifier model, its AudioSet label list).
    pub sound: Option<(PathBuf, PathBuf)>,
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
    /// Set at shutdown: a request in flight gives up within `CANCEL_POLL`.
    cancel: Arc<AtomicBool>,
    /// Tests only: run the program with these arguments instead of the model paths.
    #[cfg(test)]
    args: Option<Vec<String>>,
}

const CANCEL_POLL: Duration = Duration::from_millis(100);

const TIMEOUT_PER_AUDIO: u32 = 2;

impl Worker {
    pub fn new(config: WorkerConfig) -> Self {
        Worker {
            config,
            running: None,
            failures: 0,
            base_timeout: Duration::from_secs(30),
            cancel: Arc::new(AtomicBool::new(false)),
            #[cfg(test)]
            args: None,
        }
    }

    /// Share the engine's shutdown flag. A cancelled request kills the worker but does not count
    /// as a failure.
    pub fn set_cancel(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = cancel;
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
        if self.config.speakers.is_none() {
            return Err(Error::Other("speaker separation isn't set up".into()));
        }
        self.ask(Kind::Separate, samples)
    }

    /// Score what kind of sound `samples` is (16 kHz mono), for music rejection.
    pub fn classify(&mut self, samples: &[f32]) -> Result<Sound> {
        if self.config.sound.is_none() {
            return Err(Error::Other("the sound classifier isn't set up".into()));
        }
        self.ask(Kind::Classify, samples)?.sound.ok_or_else(|| Error::Other("no sound scores".into()))
    }

    fn ask(&mut self, kind: Kind, samples: &[f32]) -> Result<Reply> {
        if self.given_up() {
            return Err(Error::Other("the worker is unavailable".into()));
        }
        let duration_ms = samples.len() as i64 / 16;
        let result = self.request(kind, samples).and_then(|reply| {
            protocol::validate(&reply, kind, duration_ms).map_err(|e| Error::Other(e.into()))?;
            Ok(reply)
        });
        match &result {
            Ok(_) => self.failures = 0,
            Err(_) => {
                self.stop();
                if !self.cancel.load(Ordering::Acquire) {
                    self.failures += 1;
                }
            }
        }
        result
    }

    fn request(&mut self, kind: Kind, samples: &[f32]) -> Result<Reply> {
        if self.running.is_none() {
            self.running = Some(self.spawn()?);
        }
        let running = self.running.as_mut().ok_or_else(|| Error::Other("worker missing".into()))?;
        let audio = Duration::from_millis(samples.len() as u64 / 16);
        let deadline = Instant::now() + self.base_timeout + audio * TIMEOUT_PER_AUDIO;
        let cancel = &self.cancel;
        let Running { child, stdin, replies } = running;
        // The request (megabytes of audio) is written on its own thread: a worker that is alive
        // but not reading would otherwise block this one in `write` before any deadline or
        // cancellation check. Killing the child breaks the pipe and frees the writer.
        std::thread::scope(|scope| {
            let writer = scope.spawn(move || protocol::write_request(stdin, kind, samples));
            // Supervise until both the write and the reply are complete: a confused worker
            // could answer early and stop reading, leaving the write (and a join) blocked.
            let mut reply = None;
            let result = loop {
                if reply.is_some() && writer.is_finished() {
                    break Ok(());
                }
                if cancel.load(Ordering::Acquire) {
                    break Err(Error::Other("speaker separation cancelled".into()));
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    break Err(Error::Other("speaker worker timed out".into()));
                }
                if reply.is_some() {
                    std::thread::sleep(left.min(Duration::from_millis(5)));
                    continue;
                }
                match replies.recv_timeout(left.min(CANCEL_POLL)) {
                    Ok(Ok(r)) => reply = Some(r),
                    Ok(Err(_)) | Err(RecvTimeoutError::Disconnected) => {
                        break Err(Error::Other("speaker worker stopped".into()));
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                }
            };
            if result.is_err() {
                let _ = child.kill();
            }
            // The writer has finished, or the child is dead and its pipe broken: never blocks.
            let written = writer.join();
            result?;
            match (reply, written) {
                (Some(reply), Ok(Ok(()))) => Ok(reply),
                _ => Err(Error::Other("speaker worker answered before reading the request".into())),
            }
        })
    }

    fn spawn(&self) -> Result<Running> {
        let mut command = Command::new(&self.config.program);
        if let Some((segmentation, embedding)) = &self.config.speakers {
            command.arg("--segmentation").arg(segmentation).arg("--embedding").arg(embedding);
        }
        if let Some((model, labels)) = &self.config.sound {
            command.arg("--sound").arg(model).arg("--sound-labels").arg(labels);
        }
        #[cfg(test)]
        if let Some(args) = &self.args {
            command = Command::new(&self.config.program);
            command.args(args);
        }
        command
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

    #[cfg(test)]
    fn with_args(mut self, args: &[&str]) -> Self {
        self.args = Some(args.iter().map(|a| a.to_string()).collect());
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
        WorkerConfig { program: program.into(), speakers: Some(("seg.onnx".into(), "emb.onnx".into())), sound: None }
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

    /// A process that stays alive for 30 s without reading stdin or writing stdout. On Windows
    /// `waitfor` (each test needs its own signal name: two can't wait on one at once).
    fn deaf(name: &'static str) -> Option<(&'static str, Vec<&'static str>)> {
        #[cfg(windows)]
        // Waits for a signal nobody sends: no stdin reads, no stdout until it times out.
        let (program, args) = ("C:\\Windows\\System32\\waitfor.exe", vec!["/t", "30", name]);
        #[cfg(not(windows))]
        let (program, args) = ("/bin/sleep", vec!["30"]);
        #[cfg(not(windows))]
        let _ = name;
        std::path::Path::new(program).exists().then_some((program, args))
    }

    /// Ten seconds of audio is far more than a pipe buffer: the write itself blocks. The
    /// deadline must still hold, and the stuck child must be killed.
    #[test]
    fn a_worker_that_never_reads_cannot_block_the_write() {
        let Some((program, args)) = deaf("tuldmiTestBlockedWrite") else { return };
        let mut worker = Worker::new(config(program)).with_args(&args).with_timeout(Duration::from_millis(300));
        let started = std::time::Instant::now();
        assert!(worker.separate(&vec![0.0; 16_000 * 10]).is_err());
        // 300 ms base + 2 × 10 s of audio is the deadline; it must not wait for the child's 30 s.
        assert!(started.elapsed() < Duration::from_secs(25), "took {:?}", started.elapsed());
        assert!(worker.running.is_none(), "the stuck worker is stopped");
    }

    /// Shutdown frees a request stuck in the write within a moment, and isn't a failure.
    #[test]
    fn cancellation_frees_a_blocked_write() {
        let Some((program, args)) = deaf("tuldmiTestCancelledWrite") else { return };
        let mut worker = Worker::new(config(program)).with_args(&args);
        let cancel = Arc::new(AtomicBool::new(false));
        worker.set_cancel(cancel.clone());
        let flag = cancel.clone();
        let setter = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            flag.store(true, Ordering::Release);
        });
        let started = std::time::Instant::now();
        assert!(worker.separate(&vec![0.0; 16_000 * 10]).is_err());
        assert!(started.elapsed() < Duration::from_secs(5), "took {:?}", started.elapsed());
        setter.join().unwrap();
        assert_eq!(worker.failures, 0, "a cancelled request is not a worker failure");
    }
}
