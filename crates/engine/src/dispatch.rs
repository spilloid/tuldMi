//! Event dispatch: engine threads never call the shell's `EventSink` themselves.
//!
//! Every `EngineEvent` goes into one bounded in-memory queue drained by a dedicated
//! `tuldmi-events` thread, the only thread that calls `EventSink::emit`. Producers take a
//! short mutex, never wait on the sink, and never block:
//! - `Levels` are latest-wins in a single slot. They never take queue capacity, so a slow sink
//!   drops level frames before anything else.
//! - `Status` is latest-wins in a single slot (a status is a full snapshot).
//! - Everything else is queued in order up to `DISPATCH_CAPACITY`. If that is full, the new
//!   event is dropped and counted (logged once the dispatcher catches up); the engine keeps
//!   running rather than waiting on the UI.
//!
//! Shutdown closes the queue; the dispatcher delivers what is left and exits. A stuck sink
//! cannot hold up the engine: `Engine::shutdown` waits for this thread only for a short grace
//! period and otherwise abandons it.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Condvar, Mutex};

use crate::engine::EventSink;
use crate::types::{CaptureStatus, DownloadState, EngineEvent, LevelFrame};

/// Ordered (non-coalesced) events held while the sink is slow.
pub const DISPATCH_CAPACITY: usize = 1024;

#[derive(Default)]
struct Queue {
    events: VecDeque<EngineEvent>,
    status: Option<CaptureStatus>,
    levels: Option<Vec<LevelFrame>>,
    dropped: u64,
    closed: bool,
}
impl Queue {
    fn is_empty(&self) -> bool {
        self.events.is_empty() && self.status.is_none() && self.levels.is_none()
    }
}

struct Inner {
    queue: Mutex<Queue>,
    ready: Condvar,
}

/// Cheap handle; implements `EventSink` by enqueueing.
#[derive(Clone)]
pub struct Dispatcher {
    inner: Arc<Inner>,
}

impl Dispatcher {
    /// Start the dispatcher thread delivering to `sink`.
    pub fn start(
        sink: Arc<dyn EventSink>,
    ) -> crate::Result<(Dispatcher, std::thread::JoinHandle<()>)> {
        let inner = Arc::new(Inner {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
        });
        let worker = inner.clone();
        let thread = std::thread::Builder::new()
            .name("tuldmi-events".into())
            .spawn(move || run(&worker, sink.as_ref()))?;
        Ok((Dispatcher { inner }, thread))
    }

    /// Stop accepting new work once the queue is drained; the thread exits after delivering
    /// what is already queued.
    pub fn close(&self) {
        self.inner.queue.lock().closed = true;
        self.inner.ready.notify_all();
    }

    /// Wait up to `grace` for the dispatcher thread to finish; abandon it otherwise.
    pub fn finish(&self, thread: std::thread::JoinHandle<()>, grace: Duration) {
        self.close();
        let deadline = Instant::now() + grace;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        if thread.is_finished() {
            if thread.join().is_err() {
                tracing::error!("event dispatcher failed");
            }
        } else {
            tracing::warn!("event sink is not returning; abandoning undelivered events");
        }
    }
}

impl EventSink for Dispatcher {
    fn emit(&self, event: EngineEvent) {
        let mut queue = self.inner.queue.lock();
        match event {
            EngineEvent::Status(status) => queue.status = Some(status),
            EngineEvent::Levels(levels) => queue.levels = Some(levels),
            // In-flight download progress is the only droppable ordered event: coalesce it
            // with a queued progress for the same model, and drop it past capacity.
            EngineEvent::ModelProgress(p) if p.state == DownloadState::Downloading => {
                let same = queue.events.iter_mut().rev().find_map(|e| match e {
                    EngineEvent::ModelProgress(q) if q.model_id == p.model_id && q.state == DownloadState::Downloading => Some(q),
                    _ => None,
                });
                if let Some(q) = same {
                    *q = p;
                } else if queue.events.len() < DISPATCH_CAPACITY {
                    queue.events.push_back(EngineEvent::ModelProgress(p));
                } else {
                    queue.dropped += 1;
                }
            }
            // Timeline changes, notices and terminal download states are never dropped: losing
            // one leaves the UI permanently wrong (a deleted event still shown, a download stuck
            // at "downloading"). Their volume is bounded by real events, not by a sink stall.
            event => queue.events.push_back(event),
        }
        drop(queue);
        self.inner.ready.notify_one();
    }
}

fn run(inner: &Inner, sink: &dyn EventSink) {
    loop {
        let (events, status, levels, dropped) = {
            let mut queue = inner.queue.lock();
            while queue.is_empty() && !queue.closed {
                inner.ready.wait(&mut queue);
            }
            if queue.is_empty() {
                return;
            }
            (
                std::mem::take(&mut queue.events),
                queue.status.take(),
                queue.levels.take(),
                std::mem::take(&mut queue.dropped),
            )
        };
        if dropped > 0 {
            tracing::warn!(dropped, "event sink fell behind; events were dropped");
        }
        let deliver = |event: EngineEvent| {
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.emit(event))).is_err()
            {
                tracing::error!("event sink panicked");
            }
        };
        for event in events {
            deliver(event);
        }
        if let Some(status) = status {
            deliver(EngineEvent::Status(status));
        }
        if let Some(levels) = levels {
            deliver(EngineEvent::Levels(levels));
        }
    }
}
