//! Links the per-event voices the worker finds into the voices of one conversation.
//!
//! Pure and in-memory: centroids are voice fingerprints of other people, so they are never
//! persisted, and they are dropped when the conversation ends (a gap longer than
//! `CONVERSATION_GAP_MS` between events of either source), when the feature is switched off, or
//! when the app quits. Tuning comes from the AMI spike in docs/design/speakers.md.

use super::protocol::Voice;
use uuid::Uuid;

/// The timeline's conversation rule (src/lib/time.ts `CONVERSATION_GAP_MS`).
pub const CONVERSATION_GAP_MS: i64 = 3 * 60_000;
/// Cosine similarity at or above which a voice joins an existing one.
pub const MATCH_SIMILARITY: f32 = 0.5;
/// Speech a voice needs before it may start a new conversation voice (shorter embeddings are
/// too noisy to found one; they still join an existing voice they match).
pub const NEW_VOICE_MIN_MS: i64 = 1_500;
/// Speech a conversation voice needs before it gets a number and a speaker row. Keeps one-off
/// misdetections from ever showing up as a speaker.
pub const PROMOTE_MS: i64 = 10_000;

pub type VoiceId = usize;

#[derive(Debug)]
struct ConversationVoice {
    centroid: Vec<f32>,
    speech_ms: i64,
    speaker: Option<Uuid>,
    /// Segments labelled with this voice while it was too small to show, as (event, segment).
    pending: Vec<(Uuid, Uuid)>,
}

#[derive(Debug, Default)]
pub struct Tracker {
    voices: Vec<ConversationVoice>,
    /// End of the latest event seen (either source); None = no conversation yet.
    last_end: Option<i64>,
    /// Numbers already used in this conversation (continues after a restart).
    numbered: u32,
}

/// What `observe` decided about an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// Continues the current conversation.
    Continues,
    /// Starts a new conversation; the tracker was reset.
    NewConversation,
    /// Older than the current conversation (a retried event): not separated now.
    OutOfOrder,
}

impl Tracker {
    /// Report an event of either source, in transcription order. Mic events only keep the
    /// conversation alive; system events are then separated with `assign`.
    pub fn observe(&mut self, started_at: i64, ended_at: i64) -> Placement {
        match self.last_end {
            Some(last) if ended_at < last - CONVERSATION_GAP_MS => Placement::OutOfOrder,
            Some(last) if started_at - last <= CONVERSATION_GAP_MS => {
                self.last_end = Some(last.max(ended_at));
                Placement::Continues
            }
            _ => {
                self.reset();
                self.last_end = Some(ended_at);
                Placement::NewConversation
            }
        }
    }

    /// Forget every voice (feature switched off, models removed, shutdown).
    pub fn reset(&mut self) {
        *self = Tracker::default();
    }

    /// Start numbering after `used` speakers this conversation already shows (e.g. from before
    /// a restart, whose voices can't be re-linked because nothing about them was kept).
    pub fn set_numbered(&mut self, used: u32) {
        self.numbered = self.numbered.max(used);
    }

    /// Match one event's voices to the conversation. Returns, per input voice, the conversation
    /// voice it now belongs to, or None (too little speech to found a voice and no match).
    pub fn assign(&mut self, voices: &[Voice]) -> Vec<Option<VoiceId>> {
        voices
            .iter()
            .map(|voice| {
                let best = self
                    .voices
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (i, cosine(&v.centroid, &voice.embedding)))
                    .filter(|(_, s)| s.is_finite())
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match best {
                    Some((i, similarity)) if similarity >= MATCH_SIMILARITY => {
                        let v = &mut self.voices[i];
                        let (old, new) = (v.speech_ms as f32, voice.speech_ms as f32);
                        for (c, e) in v.centroid.iter_mut().zip(&voice.embedding) {
                            *c = (*c * old + e * new) / (old + new);
                        }
                        v.speech_ms += voice.speech_ms;
                        Some(i)
                    }
                    _ if voice.speech_ms >= NEW_VOICE_MIN_MS
                        && self.voices.first().is_none_or(|v| v.centroid.len() == voice.embedding.len()) =>
                    {
                        self.voices.push(ConversationVoice {
                            centroid: voice.embedding.clone(),
                            speech_ms: voice.speech_ms,
                            speaker: None,
                            pending: Vec::new(),
                        });
                        Some(self.voices.len() - 1)
                    }
                    _ => None,
                }
            })
            .collect()
    }

    /// The speaker row a voice shows as, once it has one.
    pub fn speaker(&self, voice: VoiceId) -> Option<Uuid> {
        self.voices.get(voice).and_then(|v| v.speaker)
    }

    /// Voices with enough speech to show that don't have a speaker row yet, each with the
    /// number to label it with. The caller creates the rows, then calls `promoted`.
    pub fn due(&self) -> Vec<(VoiceId, u32)> {
        let mut next = self.numbered;
        self.voices
            .iter()
            .enumerate()
            .filter(|(_, v)| v.speaker.is_none() && v.speech_ms >= PROMOTE_MS)
            .map(|(i, _)| {
                next += 1;
                (i, next)
            })
            .collect()
    }

    /// Record a voice's new speaker row. Returns the segments labelled before it was shown,
    /// which the caller relabels.
    pub fn promoted(&mut self, voice: VoiceId, number: u32, speaker: Uuid) -> Vec<(Uuid, Uuid)> {
        self.numbered = self.numbered.max(number);
        match self.voices.get_mut(voice) {
            Some(v) => {
                v.speaker = Some(speaker);
                std::mem::take(&mut v.pending)
            }
            None => Vec::new(),
        }
    }

    /// Remember a segment saved before its voice was shown.
    pub fn hold(&mut self, voice: VoiceId, event: Uuid, segment: Uuid) {
        if let Some(v) = self.voices.get_mut(voice)
            && v.speaker.is_none()
        {
            v.pending.push((event, segment));
        }
    }

    #[cfg(test)]
    fn voice_count(&self) -> usize {
        self.voices.len()
    }
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return f32::NAN;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-9)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn voice(local: u32, ms: i64, e: &[f32]) -> Voice {
        Voice { local, speech_ms: ms, embedding: e.to_vec() }
    }

    #[test]
    fn conversation_boundaries_follow_the_timeline_gap() {
        let mut t = Tracker::default();
        assert_eq!(t.observe(0, 5_000), Placement::NewConversation);
        assert_eq!(t.observe(5_000 + CONVERSATION_GAP_MS, 190_000), Placement::Continues);
        // A retried event from long before the conversation isn't linked into it.
        assert_eq!(t.observe(-10 * 60_000, -9 * 60_000), Placement::OutOfOrder);
        assert_eq!(t.observe(190_001 + CONVERSATION_GAP_MS, 400_000), Placement::NewConversation);
    }

    #[test]
    fn similar_voices_join_and_distinct_voices_split() {
        let mut t = Tracker::default();
        t.observe(0, 1);
        let a = [1.0, 0.0, 0.0];
        let b = [0.0, 1.0, 0.0];
        assert_eq!(t.assign(&[voice(0, 4_000, &a), voice(1, 4_000, &b)]), vec![Some(0), Some(1)]);
        // Close to `a`, but not identical: joins it.
        assert_eq!(t.assign(&[voice(0, 2_000, &[0.9, 0.2, 0.1])]), vec![Some(0)]);
        // Too short to found a voice and matches nothing: unlabelled.
        assert_eq!(t.assign(&[voice(0, 800, &[0.0, 0.0, 1.0])]), vec![None]);
        assert_eq!(t.voice_count(), 2);
        // A new conversation forgets every voice.
        t.observe(10 * 60_000, 10 * 60_000 + 1);
        assert_eq!(t.voice_count(), 0);
    }

    #[test]
    fn mismatched_embedding_sizes_never_found_or_join_a_voice() {
        let mut t = Tracker::default();
        assert_eq!(t.assign(&[voice(0, 4_000, &[1.0, 0.0])]), vec![Some(0)]);
        assert_eq!(t.assign(&[voice(0, 4_000, &[1.0, 0.0, 0.0])]), vec![None]);
    }

    #[test]
    fn voices_are_numbered_once_they_have_enough_speech() {
        let mut t = Tracker::default();
        t.set_numbered(2); // two speakers from before a restart
        let a = [1.0, 0.0];
        assert_eq!(t.assign(&[voice(0, 6_000, &a)]), vec![Some(0)]);
        t.hold(0, Uuid::nil(), Uuid::from_u128(1));
        assert!(t.due().is_empty(), "6 s is not enough to show");
        t.assign(&[voice(0, 5_000, &a)]);
        assert_eq!(t.due(), vec![(0, 3)]);
        let speaker = Uuid::from_u128(9);
        assert_eq!(t.promoted(0, 3, speaker), vec![(Uuid::nil(), Uuid::from_u128(1))]);
        assert_eq!(t.speaker(0), Some(speaker));
        assert!(t.due().is_empty());
        // Holding after promotion does nothing: the caller labels it directly.
        t.hold(0, Uuid::nil(), Uuid::from_u128(2));
        assert!(t.promoted(0, 3, speaker).is_empty());
    }
}
