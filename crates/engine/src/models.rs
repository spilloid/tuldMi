//! Transcription model catalog and consented downloads.
//!
//! Weights are OpenAI Whisper checkpoints (MIT) converted to ggml by the whisper.cpp project
//! (MIT), served from Hugging Face. Sizes and SHA-256 hashes were taken from the Hugging Face LFS
//! metadata on 2026-10-23 and are verified after download; a mismatch deletes the file.
//!
//! Download contract: only `ModelManager::download` starts network I/O, and only the UI's
//! explicit user action calls it. Streams to `models/<file>.part`, checks free disk space first
//! (size + 10% headroom), emits throttled (<= 10/s) `ModelProgress`, supports cancel, verifies
//! SHA-256, renames atomically, records the installation in the store. Never re-downloads an
//! installed, verified file. Network reads run on a separate fetch thread: a connection that
//! delivers no bytes for `DOWNLOAD_IDLE_TIMEOUT` fails (the user can retry), and cancel takes
//! effect within ~250 ms even during a blocked read. Shutdown cancels and briefly joins
//! downloads; a `.part` stranded anyway is swept at the next engine start.

use crate::types::ModelInfo;

pub struct CatalogEntry {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub file: &'static str,
    pub size_bytes: u64,
    pub sha256: &'static str,
    pub multilingual: bool,
    pub recommended: bool,
}

pub const HF_BASE: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/";
pub const LICENSE: &str = "MIT (OpenAI Whisper weights; ggml conversion by whisper.cpp)";

pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "whisper-tiny.en",
        name: "Whisper Tiny (English)",
        description: "Fastest and smallest. Good for older or low-power machines; less accurate.",
        file: "ggml-tiny.en.bin",
        size_bytes: 77_704_715,
        sha256: "921e4cf8686fdd993dcd081a5da5b6c365bfde1162e72b08d75ac75289920b1f",
        multilingual: false,
        recommended: false,
    },
    CatalogEntry {
        id: "whisper-base.en",
        name: "Whisper Base (English)",
        description: "Balanced speed and accuracy for English on most CPUs. Recommended.",
        file: "ggml-base.en.bin",
        size_bytes: 147_964_211,
        sha256: "a03779c86df3323075f5e796cb2ce5029f00ec8869eee3fdfb897afe36c6d002",
        multilingual: false,
        recommended: true,
    },
    CatalogEntry {
        id: "whisper-base",
        name: "Whisper Base (Multilingual)",
        description: "Same size as Base English, recognizes ~99 languages.",
        file: "ggml-base.bin",
        size_bytes: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
        multilingual: true,
        recommended: false,
    },
    CatalogEntry {
        id: "whisper-small.en",
        name: "Whisper Small (English)",
        description: "Noticeably more accurate English; needs a reasonably fast CPU.",
        file: "ggml-small.en.bin",
        size_bytes: 487_614_201,
        sha256: "c6138d6d58ecc8322097e0f987c32f1be8bb0a18532a3f88f734d1bbf9c41e5d",
        multilingual: false,
        recommended: false,
    },
    CatalogEntry {
        id: "whisper-large-v3-turbo-q5",
        name: "Whisper Large v3 Turbo (quantized)",
        description: "Highest accuracy, multilingual. Slowest on CPU; best on fast machines.",
        file: "ggml-large-v3-turbo-q5_0.bin",
        size_bytes: 574_041_195,
        sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2",
        multilingual: true,
        recommended: false,
    },
];

pub fn entry(id: &str) -> Option<&'static CatalogEntry> {
    CATALOG.iter().find(|e| e.id == id)
}

impl CatalogEntry {
    pub fn url(&self) -> String {
        format!("{HF_BASE}{}", self.file)
    }
    pub fn info(&self, installed: bool) -> ModelInfo {
        ModelInfo {
            id: self.id.into(),
            name: self.name.into(),
            description: self.description.into(),
            size_bytes: self.size_bytes,
            languages: if self.multilingual {
                "Multilingual"
            } else {
                "English"
            }
            .into(),
            license: LICENSE.into(),
            url: self.url(),
            installed,
            recommended: self.recommended,
        }
    }
}

// `ModelManager` (download/cancel/delete/verify) is implemented in Unit 2; see module docs.

use crate::{
    Error, Result,
    engine::EventSink,
    storage::{InstalledModel, Store},
    types::*,
};
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

/// A download fails when no bytes arrive for this long (the user can retry).
pub const DOWNLOAD_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// How often a download notices cancellation, even while the network read is blocked.
const CANCEL_POLL: Duration = Duration::from_millis(250);

pub struct ModelManager {
    store: Arc<Store>,
    sink: Arc<dyn EventSink>,
    downloads: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
}
impl ModelManager {
    pub fn new(store: Arc<Store>, sink: Arc<dyn EventSink>) -> Self {
        Self {
            store,
            sink,
            downloads: Arc::new(Mutex::new(HashMap::new())),
            threads: Mutex::new(Vec::new()),
        }
    }
    /// Cancel every download and wait up to `timeout` for their threads, which remove their
    /// `.part` files. Threads still running after that are left to finish on their own.
    pub fn shutdown(&self, timeout: Duration) {
        self.cancel_all();
        let deadline = Instant::now() + timeout;
        let mut threads = self.threads.lock();
        while threads.iter().any(|t| !t.is_finished()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        for thread in threads.drain(..) {
            if thread.is_finished() {
                if thread.join().is_err() {
                    tracing::error!("model download worker failed");
                }
            } else {
                tracing::warn!("model download still stopping; its partial file is swept on next start");
            }
        }
    }
    pub fn list(&self) -> Result<Vec<ModelInfo>> {
        let installed = self.store.installed_models()?;
        Ok(CATALOG
            .iter()
            .map(|e| {
                e.info(installed.iter().any(|m| {
                    m.model_id == e.id
                        && m.sha256 == e.sha256
                        && m.bytes == e.size_bytes
                        && std::fs::metadata(&m.path).is_ok_and(|file| file.len() == e.size_bytes)
                }))
            })
            .collect())
    }
    pub fn download(&self, model_id: &str) -> Result<()> {
        let entry = entry(model_id).ok_or_else(|| Error::Model("unknown model".into()))?;
        if self.list()?.iter().any(|m| m.id == model_id && m.installed) {
            return Ok(());
        }
        let mut downloads = self.downloads.lock();
        if downloads.contains_key(model_id) {
            return Ok(());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        downloads.insert(model_id.into(), cancel.clone());
        let store = self.store.clone();
        let sink = self.sink.clone();
        let active = self.downloads.clone();
        let spawned = std::thread::Builder::new()
            .name("tuldmi-download".into())
            .spawn(move || {
                let part = store
                    .data_dir()
                    .join("models")
                    .join(format!("{}.part", entry.file));
                let mut downloaded = 0;
                let progress = |state, downloaded, error| {
                    let event = EngineEvent::ModelProgress(ModelProgress {
                        model_id: entry.id.into(),
                        state,
                        downloaded,
                        total: entry.size_bytes,
                        error,
                    });
                    if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink.emit(event)))
                        .is_err()
                    {
                        tracing::error!("model progress sink panicked");
                    }
                };
                progress(DownloadState::Downloading, 0, None);
                let result = download_file(entry, &part, &cancel, &mut downloaded, |n| {
                    progress(DownloadState::Downloading, n, None)
                })
                .and_then(|hash| {
                    if cancel.load(Ordering::Acquire) {
                        return Err(Error::Model("download cancelled".into()));
                    }
                    progress(DownloadState::Verifying, downloaded, None);
                    if hash != entry.sha256 || downloaded != entry.size_bytes {
                        return Err(Error::Model("model checksum or size mismatch".into()));
                    }
                    let path = store.data_dir().join("models").join(entry.file);
                    std::fs::rename(&part, &path)?;
                    if let Err(error) = store.record_model(&InstalledModel {
                        model_id: entry.id.into(),
                        path: path.clone(),
                        bytes: downloaded,
                        sha256: hash,
                    }) {
                        let _ = std::fs::remove_file(path);
                        return Err(error);
                    }
                    Ok(())
                });
                if result.is_err() {
                    let _ = std::fs::remove_file(&part);
                }
                let cancelled = cancel.load(Ordering::Acquire);
                active.lock().remove(entry.id);
                match result {
                    Ok(()) => progress(DownloadState::Done, downloaded, None),
                    Err(_) if cancelled => progress(DownloadState::Cancelled, downloaded, None),
                    Err(error) => {
                        progress(DownloadState::Failed, downloaded, Some(error.to_string()))
                    }
                }
            });
        match spawned {
            Ok(thread) => {
                let mut threads = self.threads.lock();
                threads.retain(|t| !t.is_finished());
                threads.push(thread);
            }
            Err(error) => {
                downloads.remove(model_id);
                return Err(error.into());
            }
        }
        Ok(())
    }
    pub fn cancel(&self, model_id: &str) {
        if let Some(flag) = self.downloads.lock().get(model_id) {
            flag.store(true, Ordering::Release);
        }
    }
    pub fn cancel_all(&self) {
        for flag in self.downloads.lock().values() {
            flag.store(true, Ordering::Release);
        }
    }
    pub fn delete(&self, model_id: &str) -> Result<()> {
        if entry(model_id).is_none() {
            return Err(Error::Model("unknown model".into()));
        }
        let downloading = self.downloads.lock().contains_key(model_id);
        if downloading {
            self.cancel(model_id);
            return Err(Error::Model(
                "download cancelling; retry deletion when it finishes".into(),
            ));
        }
        for model in self.store.installed_models()? {
            if model.model_id == model_id {
                match std::fs::remove_file(&model.path) {
                    Ok(()) => (),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                    Err(e) => return Err(e.into()),
                }
            }
        }
        self.store.remove_model(model_id)
    }
    /// Verify a file without installing it or performing network I/O.
    pub fn verify(path: &Path, expected: &str) -> Result<bool> {
        let mut file = std::fs::File::open(path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 65536];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        Ok(format!("{:x}", hash.finalize()) == expected)
    }
}
impl Drop for ModelManager {
    fn drop(&mut self) {
        self.cancel_all();
    }
}
fn download_file(
    entry: &CatalogEntry,
    part: &Path,
    cancel: &AtomicBool,
    downloaded: &mut u64,
    mut progress: impl FnMut(u64),
) -> Result<String> {
    let parent = part
        .parent()
        .ok_or_else(|| Error::Model("missing models directory".into()))?;
    std::fs::create_dir_all(parent)?;
    if fs4::available_space(parent)? < entry.size_bytes + entry.size_bytes / 10 {
        return Err(Error::Model("not enough free disk space for model".into()));
    }
    // Network I/O runs on a fetch thread so cancellation is noticed within `CANCEL_POLL` even
    // while a read is blocked. Only this thread touches the `.part` file.
    let (chunks, fetched) = crossbeam_channel::bounded(16);
    let url = entry.url();
    std::thread::Builder::new()
        .name("tuldmi-fetch".into())
        .spawn(move || fetch(&url, &chunks))?;
    let mut file = std::fs::File::create(part)?;
    let mut hash = Sha256::new();
    let mut last = Instant::now();
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Model("download cancelled".into()));
        }
        let bytes = match fetched.recv_timeout(CANCEL_POLL) {
            Ok(Fetched::Chunk(bytes)) => bytes,
            Ok(Fetched::End) => break,
            Ok(Fetched::Failed(error)) => return Err(error),
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => continue,
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                return Err(Error::Model("download stopped unexpectedly".into()));
            }
        };
        *downloaded += bytes.len() as u64;
        if *downloaded > entry.size_bytes {
            return Err(Error::Model("model exceeds expected size".into()));
        }
        file.write_all(&bytes)?;
        hash.update(&bytes);
        if last.elapsed() >= Duration::from_millis(100) {
            progress(*downloaded);
            last = Instant::now();
        }
    }
    file.sync_all()?;
    Ok(format!("{:x}", hash.finalize()))
}

enum Fetched {
    Chunk(Vec<u8>),
    End,
    Failed(Error),
}

/// Stream `url` into `chunks`. The blocking client applies its timeout to the request and to
/// each body read, so a connection that delivers nothing for `DOWNLOAD_IDLE_TIMEOUT` fails.
/// Exits as soon as the receiver is gone (cancelled or failed download).
fn fetch(url: &str, chunks: &crossbeam_channel::Sender<Fetched>) {
    let result = (|| -> Result<()> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(DOWNLOAD_IDLE_TIMEOUT)
            .build()
            .map_err(|e| Error::Model(e.to_string()))?;
        let mut response = client
            .get(url)
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|e| Error::Model(e.to_string()))?;
        let mut buffer = vec![0u8; 65536];
        loop {
            let n = response.read(&mut buffer).map_err(|e| {
                Error::Model(format!("download stalled or interrupted; try again ({e})"))
            })?;
            if n == 0 {
                let _ = chunks.send(Fetched::End);
                return Ok(());
            }
            if chunks.send(Fetched::Chunk(buffer[..n].to_vec())).is_err() {
                return Ok(());
            }
        }
    })();
    if let Err(error) = result {
        let _ = chunks.send(Fetched::Failed(error));
    }
}
