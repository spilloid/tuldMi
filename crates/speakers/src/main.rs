//! `tuldmi-speakers`: separates who spoke when in a buffer of system audio.
//!
//! The engine starts this process and talks to it over stdin/stdout
//! (`tuldmi_engine::speakers::protocol`). It runs sherpa-onnx — pyannote segmentation 3.0 plus
//! NeMo TitaNet-small embeddings — which can end the process from inside C++, so it never runs in
//! the app (docs/design/speakers.md). It sees audio only, never text, writes nothing but replies
//! to stdout, and exits when stdin closes.
//!
//!   tuldmi-speakers --segmentation <model.onnx> --embedding <titanet.onnx>

use sherpa_onnx::{
    OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig, SpeakerEmbeddingExtractor,
    SpeakerEmbeddingExtractorConfig,
};
use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;
use tuldmi_engine::speakers::protocol::{self, Reply, Turn, Voice};

const RATE: usize = 16_000;
/// Speaker-clustering threshold *within* one buffer. Deliberately low: splitting one person in
/// two here is harmless (the engine's tracker merges them by voice), merging two people is not.
const LOCAL_THRESHOLD: f32 = 0.5;
/// Silence appended to every buffer: sherpa-onnx 1.13.8 aborts the process when speech runs
/// into the very end of the buffer ("This segment is too short").
const PAD_SAMPLES: usize = RATE;
/// Shortest buffer worth separating, and shortest speech worth an embedding.
const MIN_BUFFER_SAMPLES: usize = RATE / 2;
const MIN_VOICE_SAMPLES: usize = RATE / 2;
const THREADS: i32 = 2;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let (Some(segmentation), Some(embedding)) = (value("--segmentation"), value("--embedding")) else {
        eprintln!("usage: tuldmi-speakers --segmentation <model.onnx> --embedding <model.onnx>");
        return ExitCode::from(2);
    };
    let mut config = OfflineSpeakerDiarizationConfig::default();
    config.segmentation.pyannote.model = Some(segmentation);
    config.segmentation.num_threads = THREADS;
    config.embedding.model = Some(embedding.clone());
    config.embedding.num_threads = THREADS;
    config.clustering.num_clusters = -1;
    config.clustering.threshold = LOCAL_THRESHOLD;
    config.min_duration_on = 0.3;
    config.min_duration_off = 0.5;
    let Some(diarizer) = OfflineSpeakerDiarization::create(&config) else {
        eprintln!("could not load the segmentation or embedding model");
        return ExitCode::from(2);
    };
    let Some(extractor) = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
        model: Some(embedding),
        num_threads: THREADS,
        debug: false,
        provider: None,
    }) else {
        eprintln!("could not load the embedding model");
        return ExitCode::from(2);
    };

    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    loop {
        let samples = match protocol::read_request(&mut input) {
            Ok(Some(samples)) => samples,
            Ok(None) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::from(1),
        };
        let reply = separate(&diarizer, &extractor, &samples);
        if protocol::write_reply(&mut output, &reply).is_err() {
            return ExitCode::from(1);
        }
    }
}

fn separate(diarizer: &OfflineSpeakerDiarization, extractor: &SpeakerEmbeddingExtractor, samples: &[f32]) -> Reply {
    if samples.len() < MIN_BUFFER_SAMPLES {
        return Reply::default();
    }
    let mut padded = Vec::with_capacity(samples.len() + PAD_SAMPLES);
    padded.extend_from_slice(samples);
    padded.resize(samples.len() + PAD_SAMPLES, 0.0);
    // None: nothing that looks like speech (sherpa-onnx logs "No speakers found").
    let Some(result) = diarizer.process(&padded) else {
        return Reply::default();
    };
    let duration_ms = (samples.len() * 1000 / RATE) as i64;
    let turns: Vec<Turn> = result
        .sort_by_start_time()
        .into_iter()
        .filter(|g| g.speaker >= 0 && g.start.is_finite() && g.end.is_finite())
        .map(|g| Turn {
            start_ms: ((g.start * 1000.0).round() as i64).clamp(0, duration_ms),
            end_ms: ((g.end * 1000.0).round() as i64).clamp(0, duration_ms),
            local: g.speaker as u32,
        })
        .filter(|t| t.end_ms > t.start_ms)
        .collect();

    let mut locals: Vec<u32> = turns.iter().map(|t| t.local).collect();
    locals.sort_unstable();
    locals.dedup();
    let mut voices = Vec::new();
    for local in locals {
        let mut audio = Vec::new();
        for turn in turns.iter().filter(|t| t.local == local) {
            let from = (turn.start_ms as usize * RATE / 1000).min(samples.len());
            let to = (turn.end_ms as usize * RATE / 1000).min(samples.len());
            audio.extend_from_slice(&samples[from..to]);
        }
        if audio.len() < MIN_VOICE_SAMPLES {
            continue;
        }
        let Some(stream) = extractor.create_stream() else { continue };
        stream.accept_waveform(RATE as i32, &audio);
        stream.input_finished();
        if !extractor.is_ready(&stream) {
            continue;
        }
        if let Some(embedding) = extractor.compute(&stream)
            && embedding.iter().all(|x| x.is_finite())
            && embedding.iter().any(|x| *x != 0.0)
        {
            voices.push(Voice { local, speech_ms: (audio.len() * 1000 / RATE) as i64, embedding });
        }
    }
    Reply { turns, voices, error: None }
}
