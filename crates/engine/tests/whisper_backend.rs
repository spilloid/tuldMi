#![cfg(feature = "whisper")]

use std::{path::Path, time::Instant};
use tuldmi_engine::transcribe::{TranscriptionProvider, whisper::WhisperProvider};

/// Uses only the checked-in synthetic speech fixture and an explicitly supplied
/// local model. Never opens the user's capture database or saved recordings.
#[test]
#[ignore = "requires TULDMI_TEST_MODEL; optionally TULDMI_EXPECT_BACKEND=Metal/Vulkan/CPU"]
fn installed_model_backend_transcribes_fixture() {
    let model = std::env::var("TULDMI_TEST_MODEL").expect("set TULDMI_TEST_MODEL");
    let started = Instant::now();
    let mut provider = WhisperProvider::load(Path::new(&model), "fixture-model").unwrap();
    let load_ms = started.elapsed().as_millis();
    if let Ok(expected) = std::env::var("TULDMI_EXPECT_BACKEND") {
        assert!(
            matches!(expected.as_str(), "Metal" | "Vulkan" | "CPU"),
            "invalid expected backend"
        );
        assert_eq!(
            provider.backend_label(),
            Some(expected.as_str()),
            "requested backend did not initialize"
        );
    }
    if std::env::var_os("TULDMI_EXPECT_VULKAN").is_some() {
        assert_eq!(
            provider.backend_label(),
            Some("Vulkan"),
            "GPU build silently fell back"
        );
    }
    if std::env::var_os("TULDMI_FORCE_CPU").is_some() {
        assert_eq!(provider.backend_label(), Some("CPU"));
    }
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/speech.wav");
    let mut reader = hound::WavReader::open(fixture).unwrap();
    assert_eq!(reader.spec().sample_rate, 16_000);
    assert_eq!(reader.spec().channels, 1);
    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap() as f32 / 32768.0)
        .collect();
    for round in 1..=3 {
        let started = Instant::now();
        let transcript = provider.transcribe(&samples, Some("en")).unwrap();
        assert!(
            !transcript.segments.is_empty(),
            "fixture must produce real transcription"
        );
        assert!(
            transcript
                .segments
                .iter()
                .all(|s| !s.text.trim().is_empty())
        );
        // v0.4 speaker separation consumes these per-token times and bytes. GPU
        // offload must preserve the segment/piece alignment and bounded timestamps.
        assert_eq!(transcript.pieces.len(), transcript.segments.len());
        for (segment, pieces) in transcript.segments.iter().zip(&transcript.pieces) {
            assert!(!pieces.is_empty(), "speaker token pieces missing");
            assert!(
                pieces.iter().any(|piece| piece.end_ms > piece.start_ms),
                "speaker token timing must contain a real span"
            );
            assert!(pieces.iter().all(|piece| piece.start_ms >= segment.start_ms
                && piece.end_ms >= piece.start_ms
                && piece.end_ms <= segment.end_ms));
            let bytes: Vec<u8> = pieces
                .iter()
                .flat_map(|piece| piece.bytes.iter().copied())
                .collect();
            assert!(
                String::from_utf8_lossy(&bytes).trim() == segment.text.trim(),
                "speaker token pieces must preserve recognized text"
            );
        }
        let recognized = transcript
            .segments
            .iter()
            .map(|s| s.text.to_lowercase())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(recognized.contains("budget"), "fixture keyword missing");
        assert!(recognized.contains("thursday"), "fixture keyword missing");
        eprintln!(
            "round={} backend={:?} load_ms={} inference_ms={} audio_ms={} segments={}",
            round,
            provider.backend_label(),
            load_ms,
            started.elapsed().as_millis(),
            samples.len() / 16,
            transcript.segments.len()
        );
    }
}
