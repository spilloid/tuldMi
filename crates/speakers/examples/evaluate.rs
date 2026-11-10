//! Score the shipping speaker pipeline (worker process + conversation tracker) on a recording
//! with reference speaker times, e.g. an AMI meeting and its RTTM:
//!
//!   cargo run --release -p tuldmi-speakers --example evaluate -- <worker> <segmentation.onnx> \
//!       <titanet.onnx> <meeting.wav> <meeting.rttm>
//!
//! The recording is cut into "events" the way the VAD would (pauses of ≥ 400 ms, at most 30 s)
//! and fed through `Worker` and `Tracker` in order. Prints how many speakers would be shown and
//! how much reference speech landed on the right one (best one-to-one mapping).

use std::collections::HashMap;
use tuldmi_engine::speakers::{tracker::{Tracker, PROMOTE_MS}, worker::{Worker, WorkerConfig}};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let mut worker = Worker::new(WorkerConfig { program: (&a[0]).into(), segmentation: (&a[1]).into(), embedding: (&a[2]).into() });
    let mut reader = hound::WavReader::open(&a[3]).expect("wav");
    assert_eq!(reader.spec().sample_rate, 16_000);
    let audio: Vec<f32> = reader.samples::<i16>().map(|s| s.unwrap() as f32 / 32768.0).collect();
    let frames = audio.len() / 160; // 10 ms
    let mut names: Vec<String> = Vec::new();
    let mut truth = vec![-1i32; frames];
    for line in std::fs::read_to_string(&a[4]).expect("rttm").lines() {
        let w: Vec<&str> = line.split_whitespace().collect();
        let (start, dur): (f64, f64) = (w[3].parse().unwrap(), w[4].parse().unwrap());
        let k = names.iter().position(|n| n == w[7]).unwrap_or_else(|| { names.push(w[7].into()); names.len() - 1 }) as i32;
        for f in &mut truth[((start * 100.0) as usize).min(frames)..(((start + dur) * 100.0) as usize).min(frames)] {
            *f = if *f == -1 || *f == k { k } else { -3 }; // overlap: not scored
        }
    }
    // VAD-like events.
    let mut events = Vec::new();
    let (mut start, mut quiet) = (None, 0usize);
    for (i, block) in audio.chunks(512).enumerate() {
        let rms = (block.iter().map(|x| x * x).sum::<f32>() / block.len() as f32).sqrt();
        let at = i * 512;
        if rms > 0.01 { quiet = 0; start.get_or_insert(at); } else { quiet += 1; }
        if let Some(s) = start && (quiet * 512 >= 6_400 || at - s >= 30 * 16_000) {
            events.push((s, at + 512));
            start = None;
            quiet = 0;
        }
    }
    if let Some(s) = start { events.push((s, audio.len())); }

    let mut tracker = Tracker::default();
    let mut labels = vec![-1i32; frames];
    let mut voice_of_frame: Vec<(usize, usize, usize)> = Vec::new(); // frames [from,to) -> voice
    let started = std::time::Instant::now();
    let mut failures = 0;
    for &(s, e) in &events {
        let at_ms = (s / 16) as i64;
        tracker.observe(at_ms, (e / 16) as i64);
        let reply = match worker.separate(&audio[s..e]) { Ok(r) => r, Err(_) => { failures += 1; continue } };
        let voices = tracker.assign(&reply.voices);
        let by_local: HashMap<u32, Option<usize>> = reply.voices.iter().zip(&voices).map(|(v, id)| (v.local, *id)).collect();
        for turn in &reply.turns {
            let from = (s / 160 + turn.start_ms as usize / 10).min(frames);
            let to = (s / 160 + turn.end_ms as usize / 10).min(frames);
            match by_local.get(&turn.local).copied().flatten() {
                Some(v) => voice_of_frame.push((from, to, v)),
                None => for f in &mut labels[from..to] { *f = -2 },
            }
        }
        for (voice, number) in tracker.due() {
            tracker.promoted(voice, number, uuid::Uuid::nil());
        }
    }
    let elapsed = started.elapsed().as_secs_f32();
    // Final state: voices that never reached PROMOTE_MS stay "Remote" (-2).
    let mut shown = std::collections::HashSet::new();
    for &(from, to, voice) in &voice_of_frame {
        let visible = tracker.speaker(voice).is_some();
        if visible { shown.insert(voice); }
        for f in &mut labels[from..to] { *f = if visible { voice as i32 } else { -2 } }
    }
    let mut counts: HashMap<(i32, i32), usize> = HashMap::new();
    let (mut both, mut remote, mut speech) = (0, 0, 0);
    for (&t, &h) in truth.iter().zip(&labels) {
        if t < 0 { continue }
        speech += 1;
        if h >= 0 { *counts.entry((t, h)).or_default() += 1; both += 1 } else if h == -2 { remote += 1 }
    }
    let mut pairs: Vec<_> = counts.into_iter().collect();
    pairs.sort_by_key(|p| std::cmp::Reverse(p.1));
    let (mut used_t, mut used_h, mut right) = (vec![], vec![], 0);
    for ((t, h), n) in pairs {
        if !used_t.contains(&t) && !used_h.contains(&h) { used_t.push(t); used_h.push(h); right += n }
    }
    println!(
        "{} people | {} speakers shown (promote at {} s) | {:.0}% of labelled speech on the right speaker | {:.0}% left as Remote | {} events, {} worker failures | {:.0} s for {:.0} s of audio",
        names.len(), shown.len(), PROMOTE_MS / 1000, 100.0 * right as f32 / both.max(1) as f32,
        100.0 * remote as f32 / speech.max(1) as f32, events.len(), failures, elapsed, audio.len() as f32 / 16_000.0
    );
}
