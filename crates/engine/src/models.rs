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
//! installed, verified file.

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
            languages: if self.multilingual { "Multilingual" } else { "English" }.into(),
            license: LICENSE.into(),
            url: self.url(),
            installed,
            recommended: self.recommended,
        }
    }
}

// `ModelManager` (download/cancel/delete/verify) is implemented in Unit 2; see module docs.
