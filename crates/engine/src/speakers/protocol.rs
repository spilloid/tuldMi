//! Wire format between the engine and the `tuldmi-speakers` worker (docs/design/speakers.md).
//!
//! One request in flight at a time over the worker's stdin/stdout, little-endian:
//! - request: `u8` kind (`Kind`), `u32` sample count, then that many `f32` samples (16 kHz mono);
//! - reply: `u32` byte length, then that many bytes of JSON (`Reply`).
//!
//! Both sides bound what they accept, so a confused peer can't make the other allocate without
//! limit. Audio never travels anywhere but this pipe; replies carry timings and embeddings only.

use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};

/// Longest buffer the worker accepts: 10 minutes at 16 kHz. Events are capped far below this
/// (`VadSettings::max_segment_ms`, at most minutes), so a bigger request is a bug.
pub const MAX_SAMPLES: usize = 16_000 * 600;
/// Largest reply the engine reads (turns plus a few embeddings of a few hundred floats).
pub const MAX_REPLY_BYTES: usize = 4 << 20;

/// A stretch of one local speaker's speech, relative to the request's first sample.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Speaker index local to this request (0-based, not comparable across requests).
    pub local: u32,
}

/// One local speaker's voice embedding and how much speech produced it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Voice {
    pub local: u32,
    pub speech_ms: i64,
    pub embedding: Vec<f32>,
}

/// What a request asks the worker for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Who spoke when (turns + voices).
    Separate = 0,
    /// What kind of sound this is (`Reply::sound`): music rejection.
    Classify = 1,
}

/// AudioSet scores from the sound classifier, each 0..=1 (independent, not a distribution).
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sound {
    pub speech: f32,
    pub music: f32,
    /// The highest of the singing classes.
    pub singing: f32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Reply {
    pub turns: Vec<Turn>,
    /// Local speakers with enough speech for an embedding; others appear only in `turns`.
    pub voices: Vec<Voice>,
    /// Answer to a `Kind::Classify` request.
    pub sound: Option<Sound>,
    /// Set when the worker could not process the request (no turns or voices then).
    pub error: Option<String>,
}

pub fn write_request(out: &mut impl Write, kind: Kind, samples: &[f32]) -> io::Result<()> {
    if samples.len() > MAX_SAMPLES {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "audio too long for the worker"));
    }
    let mut buffer = Vec::with_capacity(5 + samples.len() * 4);
    buffer.push(kind as u8);
    buffer.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    for sample in samples {
        buffer.extend_from_slice(&sample.to_le_bytes());
    }
    out.write_all(&buffer)?;
    out.flush()
}

/// `Ok(None)` at a clean end of input (the engine closed the pipe).
pub fn read_request(input: &mut impl Read) -> io::Result<Option<(Kind, Vec<f32>)>> {
    let mut kind = [0u8; 1];
    match input.read_exact(&mut kind) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let kind = match kind[0] {
        0 => Kind::Separate,
        1 => Kind::Classify,
        _ => return Err(io::Error::new(io::ErrorKind::InvalidData, "unknown request kind")),
    };
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    let count = u32::from_le_bytes(header) as usize;
    if count > MAX_SAMPLES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "request too large"));
    }
    let mut bytes = vec![0u8; count * 4];
    input.read_exact(&mut bytes)?;
    Ok(Some((
        kind,
        bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .map(|s| if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 })
            .collect(),
    )))
}

pub fn write_reply(out: &mut impl Write, reply: &Reply) -> io::Result<()> {
    let json = serde_json::to_vec(reply).map_err(io::Error::other)?;
    if json.len() > MAX_REPLY_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "reply too large"));
    }
    out.write_all(&(json.len() as u32).to_le_bytes())?;
    out.write_all(&json)?;
    out.flush()
}

pub fn read_reply(input: &mut impl Read) -> io::Result<Reply> {
    let mut header = [0u8; 4];
    input.read_exact(&mut header)?;
    let len = u32::from_le_bytes(header) as usize;
    if len > MAX_REPLY_BYTES {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "reply too large"));
    }
    let mut json = vec![0u8; len];
    input.read_exact(&mut json)?;
    serde_json::from_slice(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

/// Check a reply against the request it answers: turns inside the audio and well-formed, every
/// voice backed by turns, embeddings finite, non-zero, and all the same size. The engine treats
/// anything else as a worker failure rather than trusting it.
pub fn validate(reply: &Reply, kind: Kind, duration_ms: i64) -> Result<(), &'static str> {
    if reply.error.is_some() {
        return Err("worker reported an error");
    }
    if kind == Kind::Classify {
        let Some(sound) = reply.sound else { return Err("no sound scores") };
        if [sound.speech, sound.music, sound.singing].iter().any(|p| !p.is_finite() || !(0.0..=1.0).contains(p)) {
            return Err("invalid sound scores");
        }
        return Ok(());
    }
    // Generous slack: the worker pads the buffer with silence, and turn edges are frame-rounded.
    let limit = duration_ms + 1_500;
    for turn in &reply.turns {
        if turn.start_ms < 0 || turn.end_ms <= turn.start_ms || turn.end_ms > limit {
            return Err("turn outside the audio");
        }
    }
    let dim = reply.voices.first().map(|v| v.embedding.len());
    for voice in &reply.voices {
        if Some(voice.embedding.len()) != dim || voice.embedding.is_empty() || voice.embedding.len() > 4_096 {
            return Err("inconsistent embedding size");
        }
        if voice.embedding.iter().any(|x| !x.is_finite()) || voice.embedding.iter().all(|x| *x == 0.0) {
            return Err("invalid embedding");
        }
        if voice.speech_ms <= 0 || !reply.turns.iter().any(|t| t.local == voice.local) {
            return Err("voice without speech");
        }
        if reply.voices.iter().filter(|v| v.local == voice.local).count() > 1 {
            return Err("duplicate voice");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip_sanitizes_samples() {
        let mut wire = Vec::new();
        write_request(&mut wire, Kind::Classify, &[0.25, -2.0, f32::NAN]).unwrap();
        let (kind, samples) = read_request(&mut wire.as_slice()).unwrap().unwrap();
        assert_eq!((kind, samples), (Kind::Classify, vec![0.25, -1.0, 0.0]));
        assert_eq!(read_request(&mut [].as_slice()).unwrap(), None);
        assert!(read_request(&mut [7u8, 0, 0, 0, 0].as_slice()).is_err(), "unknown kinds are refused");
    }

    #[test]
    fn oversized_frames_are_refused_before_allocation() {
        let mut header = vec![0u8];
        header.extend(((MAX_SAMPLES + 1) as u32).to_le_bytes());
        assert!(read_request(&mut header.as_slice()).is_err());
        let header = ((MAX_REPLY_BYTES + 1) as u32).to_le_bytes();
        assert!(read_reply(&mut header.as_slice()).is_err());
        assert!(write_request(&mut Vec::new(), Kind::Separate, &vec![0.0; MAX_SAMPLES + 1]).is_err());
    }

    #[test]
    fn reply_round_trip_and_validation() {
        let reply = Reply {
            turns: vec![Turn { start_ms: 0, end_ms: 900, local: 0 }, Turn { start_ms: 1_000, end_ms: 1_800, local: 1 }],
            voices: vec![Voice { local: 0, speech_ms: 900, embedding: vec![0.5, 0.1] }],
            sound: None,
            error: None,
        };
        let mut wire = Vec::new();
        write_reply(&mut wire, &reply).unwrap();
        let back = read_reply(&mut wire.as_slice()).unwrap();
        assert_eq!(back, reply);
        assert_eq!(validate(&back, Kind::Separate, 2_000), Ok(()));

        let mut bad = reply.clone();
        bad.turns[1].end_ms = 9_000;
        assert!(validate(&bad, Kind::Separate, 2_000).is_err());
        let mut bad = reply.clone();
        bad.voices[0].embedding = vec![f32::INFINITY, 0.0];
        assert!(validate(&bad, Kind::Separate, 2_000).is_err());
        let mut bad = reply.clone();
        bad.voices.push(Voice { local: 1, speech_ms: 800, embedding: vec![1.0] });
        assert!(validate(&bad, Kind::Separate, 2_000).is_err(), "embedding sizes must agree");
        let mut bad = reply.clone();
        bad.voices[0].local = 7;
        assert!(validate(&bad, Kind::Separate, 2_000).is_err(), "a voice needs turns");
        let mut bad = reply;
        bad.error = Some("x".into());
        assert!(validate(&bad, Kind::Separate, 2_000).is_err());
    }

    #[test]
    fn classify_replies_need_sane_scores() {
        let ok = Reply { sound: Some(Sound { speech: 0.9, music: 0.0, singing: 0.0 }), ..Reply::default() };
        assert_eq!(validate(&ok, Kind::Classify, 1_000), Ok(()));
        assert!(validate(&Reply::default(), Kind::Classify, 1_000).is_err());
        let bad = Reply { sound: Some(Sound { speech: f32::NAN, music: 0.0, singing: 0.0 }), ..Reply::default() };
        assert!(validate(&bad, Kind::Classify, 1_000).is_err());
        let bad = Reply { sound: Some(Sound { speech: 1.5, music: 0.0, singing: 0.0 }), ..Reply::default() };
        assert!(validate(&bad, Kind::Classify, 1_000).is_err());
    }
}
