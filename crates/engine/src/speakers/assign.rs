//! Gives transcript segments to speakers, splitting a Whisper segment where the voice changes.
//!
//! Pure. Times are milliseconds from the event's first sample, the same base Whisper and the
//! worker use. A "label" is a conversation voice, or None for speech nobody can be sure of
//! (which shows as Remote).

use super::tracker::VoiceId;
use crate::storage::NewSegment;

/// A worker turn after the tracker mapped its local speaker to a conversation voice.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Span {
    pub start_ms: i64,
    pub end_ms: i64,
    pub voice: Option<VoiceId>,
}

/// One Whisper text token with its timing.
#[derive(Debug, Clone, PartialEq)]
pub struct Piece {
    pub start_ms: i64,
    pub end_ms: i64,
    /// Raw token bytes; a multi-byte character may span tokens, so text is decoded per run.
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Labelled {
    pub segment: NewSegment,
    pub voice: Option<VoiceId>,
}

/// Share one label needs to take a whole segment: of the segment's overlapped speech, and of at
/// least half its length (so a segment mostly outside every turn isn't given to whoever covers
/// a sliver of it; Whisper segments include pauses, which turns don't).
pub const WHOLE_SEGMENT_SHARE: f64 = 0.6;
/// Another label needs this much overlap (and `SPLIT_MIN_SHARE` of it) before a segment splits.
const SPLIT_MIN_MS: i64 = 1_000;
const SPLIT_MIN_SHARE: f64 = 0.2;
/// Runs shorter than this (and of a single word) merge into their neighbour.
const MIN_RUN_MS: i64 = 600;

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

/// Overlap per label for the interval, largest first. Empty when no span touches it.
fn shares(start: i64, end: i64, spans: &[Span]) -> Vec<(Option<VoiceId>, i64)> {
    let mut out: Vec<(Option<VoiceId>, i64)> = Vec::new();
    for span in spans {
        let ms = overlap((start, end), (span.start_ms, span.end_ms));
        if ms == 0 {
            continue;
        }
        match out.iter_mut().find(|(v, _)| *v == span.voice) {
            Some((_, total)) => *total += ms,
            None => out.push((span.voice, ms)),
        }
    }
    out.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    out
}

/// The label that covers at least `share` of the interval's overlapped speech and of half its
/// length (so at least 30% of it at the default share), else None.
pub fn majority(start: i64, end: i64, spans: &[Span], share: f64) -> Option<VoiceId> {
    let shares = shares(start, end, spans);
    let total: i64 = shares.iter().map(|(_, ms)| ms).sum();
    let base = total.max((end - start) / 2);
    match shares.first() {
        Some(&(voice, ms)) if base > 0 && ms as f64 >= share * base as f64 => voice,
        _ => None,
    }
}

/// Label each segment, splitting it at word boundaries where the voice clearly changes.
/// `pieces[i]` are segment i's tokens (may be empty, e.g. when re-labelling stored segments).
pub fn label(segments: &[NewSegment], pieces: &[Vec<Piece>], spans: &[Span]) -> Vec<Labelled> {
    let mut out = Vec::new();
    for (i, segment) in segments.iter().enumerate() {
        let shares = shares(segment.start_ms, segment.end_ms, spans);
        let total: i64 = shares.iter().map(|(_, ms)| ms).sum();
        let contested = shares
            .iter()
            .skip(1)
            .any(|&(_, ms)| ms >= SPLIT_MIN_MS && ms as f64 >= SPLIT_MIN_SHARE * total as f64);
        let tokens = pieces.get(i).map(Vec::as_slice).unwrap_or(&[]);
        if contested
            && let Some(split) = split(segment, tokens, spans)
        {
            out.extend(split);
            continue;
        }
        out.push(Labelled {
            segment: segment.clone(),
            voice: majority(segment.start_ms, segment.end_ms, spans, WHOLE_SEGMENT_SHARE),
        });
    }
    out
}

struct Word {
    start_ms: i64,
    end_ms: i64,
    bytes: Vec<u8>,
    voice: Option<Option<VoiceId>>,
}

fn split(segment: &NewSegment, tokens: &[Piece], spans: &[Span]) -> Option<Vec<Labelled>> {
    // Words start at a token beginning with a space (Whisper's word boundary), so a split never
    // lands inside a word or a multi-byte character.
    let mut words: Vec<Word> = Vec::new();
    for token in tokens {
        let starts_word = token.bytes.first() == Some(&b' ') || words.is_empty();
        if starts_word {
            words.push(Word { start_ms: token.start_ms, end_ms: token.end_ms, bytes: Vec::new(), voice: None });
        }
        let word = words.last_mut()?;
        word.end_ms = word.end_ms.max(token.end_ms);
        word.bytes.extend_from_slice(&token.bytes);
    }
    if words.len() < 2 {
        return None;
    }
    for word in &mut words {
        let start = word.start_ms.clamp(segment.start_ms, segment.end_ms);
        let end = word.end_ms.clamp(start, segment.end_ms);
        word.voice = shares(start, end.max(start + 1), spans).first().map(|&(v, _)| v);
    }
    // Words no span touches take their neighbour's label (earlier first, else later).
    let mut last = None;
    for word in &mut words {
        match word.voice {
            Some(v) => last = Some(v),
            None => word.voice = last,
        }
    }
    let first_known = words.iter().find_map(|w| w.voice);
    for word in &mut words {
        word.voice = word.voice.or(first_known);
    }

    // Runs of one label; then fold runs too short to trust into the previous (or next) run.
    let mut runs: Vec<(Option<VoiceId>, Vec<Word>)> = Vec::new();
    for word in words {
        let voice = word.voice.flatten();
        match runs.last_mut() {
            Some((v, run)) if *v == voice => run.push(word),
            _ => runs.push((voice, vec![word])),
        }
    }
    let mut i = 0;
    while runs.len() > 1 && i < runs.len() {
        let (_, run) = &runs[i];
        let ms = run.last().map_or(0, |w| w.end_ms) - run.first().map_or(0, |w| w.start_ms);
        if run.len() == 1 && ms < MIN_RUN_MS {
            let (_, words) = runs.remove(i);
            let into = if i > 0 { i - 1 } else { 0 };
            if into < i {
                runs[into].1.extend(words);
            } else {
                let mut words = words;
                words.append(&mut runs[into].1);
                runs[into].1 = words;
            }
            // Neighbours may now share a label: merge them.
            if into + 1 < runs.len() && runs[into].0 == runs[into + 1].0 {
                let (_, next) = runs.remove(into + 1);
                runs[into].1.extend(next);
            }
            i = into;
            continue;
        }
        i += 1;
    }
    if runs.len() < 2 {
        return None;
    }
    let mut out = Vec::new();
    for (voice, run) in runs {
        let bytes: Vec<u8> = run.iter().flat_map(|w| w.bytes.iter().copied()).collect();
        let text = String::from_utf8_lossy(&bytes).trim().to_owned();
        if text.is_empty() {
            continue;
        }
        let start_ms = run.first().map_or(segment.start_ms, |w| w.start_ms).clamp(segment.start_ms, segment.end_ms);
        let end_ms = run.last().map_or(segment.end_ms, |w| w.end_ms).clamp(start_ms, segment.end_ms);
        out.push(Labelled {
            segment: NewSegment { start_ms, end_ms, text, confidence: segment.confidence, speaker_id: None },
            voice,
        });
    }
    (out.len() > 1).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_ms: i64, end_ms: i64, text: &str) -> NewSegment {
        NewSegment { start_ms, end_ms, text: text.into(), confidence: Some(0.9), speaker_id: None }
    }
    fn span(start_ms: i64, end_ms: i64, voice: Option<VoiceId>) -> Span {
        Span { start_ms, end_ms, voice }
    }
    fn tokens(words: &[(&str, i64, i64)]) -> Vec<Piece> {
        words.iter().map(|&(t, s, e)| Piece { start_ms: s, end_ms: e, bytes: t.as_bytes().to_vec() }).collect()
    }

    #[test]
    fn a_segment_inside_one_turn_takes_that_voice() {
        let out = label(&[seg(0, 2_000, "hello there")], &[vec![]], &[span(0, 3_000, Some(1))]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].voice, Some(1));
        assert_eq!(out[0].segment.text, "hello there");
    }

    #[test]
    fn untouched_or_unclear_speech_stays_unlabelled() {
        let out = label(&[seg(0, 2_000, "x")], &[vec![]], &[span(5_000, 6_000, Some(0))]);
        assert_eq!(out[0].voice, None);
        // Two voices about evenly, no tokens to split on: neither is sure enough.
        let spans = [span(0, 1_000, Some(0)), span(1_000, 2_000, Some(1))];
        assert_eq!(label(&[seg(0, 2_000, "x")], &[vec![]], &spans)[0].voice, None);
        assert_eq!(majority(0, 2_000, &spans, WHOLE_SEGMENT_SHARE), None);
        assert_eq!(majority(0, 1_500, &spans, WHOLE_SEGMENT_SHARE), Some(0));
        // One second of a ten-second segment is not "most of it", whoever else is silent.
        let sliver = [span(0, 1_000, Some(0))];
        assert_eq!(majority(0, 10_000, &sliver, WHOLE_SEGMENT_SHARE), None);
        assert_eq!(majority(0, 3_000, &[span(0, 1_000, Some(0))], WHOLE_SEGMENT_SHARE), Some(0));
    }

    #[test]
    fn a_voice_change_splits_at_a_word_boundary() {
        let words = tokens(&[
            (" So", 0, 300),
            (" we", 300, 600),
            (" ship", 600, 1_000),
            (" Friday", 1_000, 1_600),
            (" Sounds", 2_000, 2_500),
            (" good", 2_500, 2_900),
            ("ness", 2_900, 3_100), // continues "good" — never split inside a word
            (" me", 3_100, 3_600),
        ]);
        let spans = [span(0, 1_800, Some(0)), span(1_800, 3_700, Some(1))];
        let out = label(&[seg(0, 3_700, "So we ship Friday Sounds goodness me")], &[words], &spans);
        assert_eq!(out.len(), 2);
        assert_eq!((out[0].voice, out[0].segment.text.as_str()), (Some(0), "So we ship Friday"));
        assert_eq!((out[1].voice, out[1].segment.text.as_str()), (Some(1), "Sounds goodness me"));
        assert_eq!((out[0].segment.start_ms, out[0].segment.end_ms), (0, 1_600));
        assert_eq!((out[1].segment.start_ms, out[1].segment.end_ms), (2_000, 3_600));
        assert_eq!(out[1].segment.confidence, Some(0.9));
    }

    #[test]
    fn a_single_stray_word_does_not_split() {
        let words = tokens(&[(" one", 0, 800), (" two", 800, 1_600), (" three", 1_600, 1_900), (" four", 1_900, 3_000)]);
        // Voice 1 owns 1.6–2.9 s only by a blip around "three", but with enough total overlap
        // to count as contested; the one-word run folds back into voice 0.
        let spans = [span(0, 1_600, Some(0)), span(1_600, 2_000, Some(1)), span(2_000, 3_000, Some(0)), span(1_000, 1_700, Some(1))];
        let out = label(&[seg(0, 3_000, "one two three four")], &[words], &spans);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn multibyte_text_survives_token_splits() {
        // "é" split across two tokens; the split happens only at the space before "oui".
        let mut words = tokens(&[(" caf", 0, 400)]);
        words.push(Piece { start_ms: 400, end_ms: 600, bytes: vec![0xC3] });
        words.push(Piece { start_ms: 600, end_ms: 900, bytes: vec![0xA9] });
        words.extend(tokens(&[(" s'il", 900, 1_400), (" oui", 2_000, 2_600), (" merci", 2_600, 3_400)]));
        let spans = [span(0, 1_500, Some(0)), span(1_900, 3_500, Some(1))];
        let out = label(&[seg(0, 3_500, "café s'il oui merci")], &[words], &spans);
        assert_eq!(out.iter().map(|l| l.segment.text.as_str()).collect::<Vec<_>>(), ["café s'il", "oui merci"]);
    }
}
