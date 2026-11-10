//! `tuldmi-speakers`: separates who spoke when in a buffer of audio, and tells music from speech.
//!
//! The engine starts this process and talks to it over stdin/stdout
//! (`tuldmi_engine::speakers::protocol`). It runs sherpa-onnx — pyannote segmentation 3.0 plus
//! NeMo TitaNet-small embeddings for speakers, CED-tiny (AudioSet) for music rejection — which
//! can end the process from inside C++, so it never runs in the app (docs/design/speakers.md).
//! It sees audio only, never text, writes nothing but replies to stdout, and exits when stdin
//! closes. Each capability is loaded only when its models are given; at least one is required.
//!
//!   tuldmi-speakers [--segmentation <model.onnx> --embedding <titanet.onnx>]
//!                   [--sound <ced.onnx> --sound-labels <class_labels_indices.csv>]

use sherpa_onnx::{
    AudioTagging, AudioTaggingConfig, OfflineSpeakerDiarization, OfflineSpeakerDiarizationConfig,
    SpeakerEmbeddingExtractor, SpeakerEmbeddingExtractorConfig,
};
use std::io::{self, BufReader, BufWriter};
use std::process::ExitCode;
use tuldmi_engine::speakers::protocol::{self, Kind, Reply, Sound, Turn, Voice};

/// AudioSet class indices (class_labels_indices.csv).
const SPEECH: i32 = 0;
const MUSIC: i32 = 137;
const SINGING: [i32; 5] = [27, 32, 33, 34, 35];
/// The classifier was trained on 10 s clips: longer events are scored in 10 s windows.
const SOUND_WINDOW: usize = 10 * RATE;

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
    if args.first().is_some_and(|a| a == "--version") {
        println!("tuldmi-speakers {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let value = |name: &str| args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned();
    let speakers = match (value("--segmentation"), value("--embedding")) {
        (Some(segmentation), Some(embedding)) => match load_speakers(segmentation, embedding) {
            Some(loaded) => Some(loaded),
            None => return ExitCode::from(2),
        },
        _ => None,
    };
    let tagger = match (value("--sound"), value("--sound-labels")) {
        (Some(model), Some(labels)) => {
            let mut config = AudioTaggingConfig::default();
            config.model.ced = Some(model);
            config.model.num_threads = THREADS;
            config.labels = Some(labels);
            config.top_k = 527;
            match AudioTagging::create(&config) {
                Some(tagger) => Some(tagger),
                None => {
                    eprintln!("could not load the sound classifier");
                    return ExitCode::from(2);
                }
            }
        }
        _ => None,
    };
    if speakers.is_none() && tagger.is_none() {
        eprintln!("usage: tuldmi-speakers [--segmentation <model.onnx> --embedding <model.onnx>] [--sound <model.onnx> --sound-labels <labels.csv>]");
        return ExitCode::from(2);
    }

    let mut input = BufReader::new(io::stdin().lock());
    let mut output = BufWriter::new(io::stdout().lock());
    loop {
        let (kind, samples) = match protocol::read_request(&mut input) {
            Ok(Some(request)) => request,
            Ok(None) => return ExitCode::SUCCESS,
            Err(_) => return ExitCode::from(1),
        };
        let reply = match (kind, &speakers, &tagger) {
            (Kind::Separate, Some((diarizer, extractor)), _) => separate(diarizer, extractor, &samples),
            (Kind::Classify, _, Some(tagger)) => match classify(tagger, &samples) {
                Some(sound) => Reply { sound: Some(sound), ..Reply::default() },
                // Never guess: the engine transcribes the event as usual.
                None => Reply { error: Some("classification failed".into()), ..Reply::default() },
            },
            _ => Reply { error: Some("not loaded".into()), ..Reply::default() },
        };
        if protocol::write_reply(&mut output, &reply).is_err() {
            return ExitCode::from(1);
        }
    }
}

/// The highest score of each class over the event, in windows of at most 10 s. Taking the
/// maximum is the cautious choice: any window with speech keeps the event as speech. None if any
/// window couldn't be scored completely (a missing class or a non-finite score): a window that
/// failed must never count as "no speech".
fn classify(tagger: &AudioTagging, samples: &[f32]) -> Option<Sound> {
    let mut sound = Sound::default();
    for window in samples.chunks(SOUND_WINDOW) {
        let mut audio = window.to_vec();
        audio.resize(audio.len().max(RATE), 0.0); // at least 1 s
        let stream = tagger.create_stream();
        stream.accept_waveform(RATE as i32, &audio);
        let events = tagger.compute(&stream, 527);
        let p = |i: i32| events.iter().find(|e| e.index == i).map(|e| e.prob).filter(|p| p.is_finite() && (0.0..=1.0).contains(p));
        sound.speech = sound.speech.max(p(SPEECH)?);
        sound.music = sound.music.max(p(MUSIC)?);
        let mut singing = 0.0f32;
        for i in SINGING {
            singing = singing.max(p(i)?);
        }
        sound.singing = sound.singing.max(singing);
    }
    Some(sound)
}

fn load_speakers(segmentation: String, embedding: String) -> Option<(OfflineSpeakerDiarization, SpeakerEmbeddingExtractor)> {
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
        return None;
    };
    let Some(extractor) = SpeakerEmbeddingExtractor::create(&SpeakerEmbeddingExtractorConfig {
        model: Some(embedding),
        num_threads: THREADS,
        debug: false,
        provider: None,
    }) else {
        eprintln!("could not load the embedding model");
        return None;
    };
    Some((diarizer, extractor))
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
    Reply { turns, voices, sound: None, error: None }
}
