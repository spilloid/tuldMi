//! tuldMi engine: capture -> resample -> VAD -> segment -> Opus + SQLite -> transcription.
//!
//! Nothing in this crate depends on Tauri or a UI. The app shell (`src-tauri`) owns an
//! `Engine` and forwards its `EngineEvent`s to the webview.

pub mod clock;
pub mod dsp;
pub mod encode;
pub mod segmenter;
pub mod storage;
pub mod types;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("i/o: {0}")]
    Io(#[from] std::io::Error),
    #[error("audio codec: {0}")]
    Codec(String),
    #[error("audio device: {0}")]
    Device(String),
    #[error("transcription: {0}")]
    Transcription(String),
    #[error("model: {0}")]
    Model(String),
    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
