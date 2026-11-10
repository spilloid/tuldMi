//! Remote speaker separation on the transcription thread (docs/design/speakers.md).
//!
//! Live: every claimed job is reported to the conversation tracker (mic events keep a
//! conversation going); a system event's samples go to the worker after Whisper, and its
//! segments are saved with speakers in the same transaction as the transcript. A worker failure
//! never costs the transcript: it is saved unlabelled with `speakers_state = 'failed'`.
//!
//! Backfill ("Separate past recordings"): when no transcription job is waiting, one stored
//! system event at a time, conversation by conversation, each with a fresh tracker. Nothing is
//! written until the whole conversation is done, then all of it in one transaction, so an
//! interrupted conversation stays eligible. Stored segments keep their text; each goes to the
//! speaker that covers most of it.
//!
//! Voice embeddings live only in the trackers here. They are dropped when the conversation ends
//! (`housekeep` also ends it while idle), when the feature is switched off (checked on every
//! loop of the transcription thread), and at shutdown. Speaker rows are created under the
//! persist gate right before the segments that reference them are written, and every
//! announcement of a changed event happens under the gate too, so it can't overtake a deletion.

use super::runtime::Shared;
use crate::models::{SPEAKER_CATALOG, installed_path};
use crate::speakers::{
    assign::{self, Labelled, Span},
    numbered_label,
    protocol::Reply,
    tracker::{Placement, Tracker, VoiceId},
    worker::{Worker, WorkerConfig},
};
use crate::storage::{BackfillEvent, Job, NewSegment, SpeakersState};
use crate::transcribe::Transcript;
use crate::types::*;
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use uuid::Uuid;

pub(super) struct Separation {
    worker: Option<Worker>,
    live: Tracker,
    /// Set when the live tracker starts a conversation (any source): the next system event
    /// looks up the numbers that conversation already shows (after a restart, say).
    number_from: Option<i64>,
    backfill: Option<Backfill>,
    gave_up_noticed: bool,
}

/// One backfilled event: its segments' voices and its outcome.
type EventResult = (Uuid, Vec<(Uuid, Option<VoiceId>)>, SpeakersState);

struct Backfill {
    /// Start of the last event of the conversation being worked on (resume point).
    cursor: i64,
    /// Remaining events of that conversation, last first.
    pending: Vec<BackfillEvent>,
    tracker: Tracker,
    /// Per event already processed: its segments' voices and its outcome. Saved together.
    results: Vec<EventResult>,
    conversations: u32,
}

/// What a live separation decided, to be saved under the persist gate.
#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
pub(super) struct Separated {
    labelled: Vec<Labelled>,
    state: SpeakersState,
}

impl Separation {
    pub fn new() -> Self {
        Separation { worker: None, live: Tracker::default(), number_from: None, backfill: None, gave_up_noticed: false }
    }

    /// Call on every loop of the transcription thread. Switching the feature off forgets every
    /// voice and stops the worker and any backfill at once, not at the next system event. While
    /// idle (`idle`: nothing waits for transcription), a conversation that can no longer
    /// continue is ended, dropping its voices.
    pub fn housekeep(&mut self, shared: &Shared, idle: bool) {
        if !shared.settings.lock().speakers_enabled {
            self.live.reset();
            self.number_from = None;
            self.worker = None;
            if self.backfill.take().is_some() | shared.backfill_requested.swap(false, Ordering::AcqRel) {
                set_backfill(shared, None);
            }
            set_state(shared, SeparationState::Off);
            return;
        }
        if idle && self.live.expire_idle(chrono::Local::now().timestamp_millis()) {
            self.number_from = None;
        }
    }

    /// Whether separation can run now; keeps the worker in step with settings and models and
    /// publishes the state. Losing a model forgets every voice.
    fn ready(&mut self, shared: &Shared) -> bool {
        let state = self.check(shared);
        if state != SeparationState::Ready {
            self.live.reset();
            self.number_from = None;
            if state != SeparationState::Unavailable {
                // Keep a given-up worker so it stays given up for the session.
                self.worker = None;
            }
            if self.backfill.take().is_some() | shared.backfill_requested.swap(false, Ordering::AcqRel) {
                set_backfill(shared, None);
            }
        }
        set_state(shared, state);
        state == SeparationState::Ready
    }

    fn check(&mut self, shared: &Shared) -> SeparationState {
        if !shared.settings.lock().speakers_enabled {
            return SeparationState::Off;
        }
        let Some(program) = shared.config.speaker_worker.clone().filter(|p| p.is_file()) else {
            return SeparationState::Unavailable;
        };
        let Ok(installed) = shared.store.installed_models() else {
            return SeparationState::NeedsModels;
        };
        let paths: Option<Vec<_>> = SPEAKER_CATALOG.iter().map(|e| installed_path(e, &installed)).collect();
        let Some([segmentation, embedding]) = paths.and_then(|p| <[_; 2]>::try_from(p).ok()) else {
            return SeparationState::NeedsModels;
        };
        let config = WorkerConfig { program, segmentation, embedding };
        if self.worker.as_ref().map(Worker::config) != Some(&config) {
            let mut worker = Worker::new(config);
            worker.set_cancel(shared.cancel_asr.clone());
            self.worker = Some(worker);
            self.gave_up_noticed = false;
        }
        if self.worker.as_ref().is_some_and(Worker::given_up) {
            if !std::mem::replace(&mut self.gave_up_noticed, true) {
                shared.notice("speaker separation stopped after repeated failures; transcripts are unaffected. Restart tuldMi to try again");
            }
            return SeparationState::Unavailable;
        }
        SeparationState::Ready
    }

    fn separate_samples(&mut self, shared: &Shared, samples: &[f32]) -> Option<Reply> {
        let worker = self.worker.as_mut()?;
        set_state(shared, SeparationState::Working);
        let reply = worker.separate(samples).ok();
        set_state(shared, if worker.given_up() { SeparationState::Unavailable } else { SeparationState::Ready });
        reply
    }

    /// Report a claimed job, in transcription order (both sources).
    #[cfg_attr(not(feature = "whisper"), allow(dead_code))]
    pub fn observe(&mut self, job: &Job) -> Placement {
        let placement = self.live.observe(job.started_at, job.ended_at);
        if placement == Placement::NewConversation {
            self.number_from = Some(job.started_at);
        }
        placement
    }

    #[cfg_attr(not(feature = "whisper"), allow(dead_code))]
    /// Separate a transcribed system event. None = save it as before (not a system event,
    /// feature off or not ready, nothing transcribed, or out of conversation order).
    pub fn separate_live(
        &mut self,
        shared: &Shared,
        job: &Job,
        placement: Placement,
        samples: &[f32],
        transcript: &Transcript,
    ) -> Option<Separated> {
        if job.source != Source::System || placement == Placement::OutOfOrder || transcript.segments.is_empty() {
            return None;
        }
        if !self.ready(shared) {
            return None;
        }
        if let Some(from) = self.number_from.take() {
            // After a restart mid-conversation the voices are gone (never stored): continue
            // numbering past the speakers this conversation already shows.
            let used = shared
                .store
                .conversation_start(from)
                .and_then(|start| shared.store.max_speaker_ordinal(start, job.started_at))
                .unwrap_or(0);
            self.live.set_numbered(used);
        }
        let reply = self.separate_samples(shared, samples);
        // Switched off while the worker ran: don't label with voices the user just dismissed.
        if !shared.settings.lock().speakers_enabled {
            return None;
        }
        let Some(reply) = reply else {
            return Some(Separated {
                labelled: transcript.segments.iter().map(|s| Labelled { segment: s.clone(), voice: None }).collect(),
                state: SpeakersState::Failed,
            });
        };
        let spans = spans(&mut self.live, &reply);
        Some(Separated { labelled: assign::label(&transcript.segments, &transcript.pieces, &spans), state: SpeakersState::Done })
    }

    #[cfg_attr(not(feature = "whisper"), allow(dead_code))]
    /// Save a separated transcript (call under the persist gate). Falls back to a plain save,
    /// marked failed, if the labelled save fails, so speakers never cost a transcript.
    /// Returns the saved event and other events whose segments were relabelled.
    pub fn commit_live(
        &mut self,
        shared: &Shared,
        job: &Job,
        separated: Separated,
        language: Option<&str>,
        model_id: &str,
    ) -> crate::Result<(SpeechEvent, Vec<Uuid>)> {
        let created = create_due(shared, &self.live);
        let speaker = |voice: Option<VoiceId>, live: &Tracker| {
            voice.and_then(|v| live.speaker(v).or_else(|| created.iter().find(|c| c.0 == v).map(|c| c.2)))
        };
        let segments: Vec<NewSegment> = separated
            .labelled
            .iter()
            .map(|l| NewSegment { speaker_id: speaker(l.voice, &self.live), ..l.segment.clone() })
            .collect();
        match shared.store.save_transcript_with_speakers(job.event_id, &segments, language, model_id, separated.state) {
            Ok((event, ids)) => {
                let relabelled = promote(shared, &mut self.live, &created);
                for (labelled, id) in separated.labelled.iter().zip(ids) {
                    if let Some(voice) = labelled.voice {
                        self.live.hold(voice, job.event_id, id);
                    }
                }
                Ok((event, relabelled))
            }
            Err(_) => {
                drop_created(shared, &created);
                // Usually a speaker row this conversation relies on was deleted with its events:
                // start its voices over rather than failing every later save the same way.
                self.live.forget_voices();
                let plain: Vec<NewSegment> =
                    separated.labelled.into_iter().map(|l| NewSegment { speaker_id: None, ..l.segment }).collect();
                shared
                    .store
                    .save_transcript_with_speakers(job.event_id, &plain, language, model_id, SpeakersState::Failed)
                    .map(|(event, _)| (event, Vec::new()))
            }
        }
    }

    // ---- backfill ---------------------------------------------------------------------------

    /// True while there is backfill work (requested or under way).
    pub fn backfilling(&self, shared: &Shared) -> bool {
        self.backfill.is_some() || shared.backfill_requested.load(Ordering::Acquire)
    }

    /// Do one stored event of backfill work. Call only when no transcription job is waiting.
    pub fn backfill_step(&mut self, shared: &Shared) {
        if !self.ready(shared) {
            return;
        }
        if shared.backfill_requested.swap(false, Ordering::AcqRel) && self.backfill.is_none() {
            self.backfill = Some(Backfill {
                cursor: i64::MIN,
                pending: Vec::new(),
                tracker: Tracker::default(),
                results: Vec::new(),
                conversations: 0,
            });
        }
        let Some(mut backfill) = self.backfill.take() else { return };
        set_backfill(shared, shared.store.backfill_candidates().ok());
        if backfill.pending.is_empty() {
            match shared.store.next_backfill_conversation(backfill.cursor) {
                Ok(Some((resume, mut events))) => {
                    backfill.cursor = resume;
                    // A qualifying conversation has no separated event, so numbering starts at 1.
                    backfill.tracker = Tracker::default();
                    backfill.results.clear();
                    events.reverse();
                    backfill.pending = events;
                    backfill.conversations += 1;
                }
                Ok(None) => {
                    set_backfill(shared, None);
                    shared.emit(EngineEvent::Notice {
                        level: NoticeLevel::Info,
                        message: match backfill.conversations {
                            0 => "No past conversations need their speakers separated".into(),
                            1 => "Separated the speakers in 1 past conversation".into(),
                            n => format!("Separated the speakers in {n} past conversations"),
                        },
                    });
                    return;
                }
                Err(_) => {
                    set_backfill(shared, None);
                    shared.notice("could not read past recordings to separate speakers");
                    return;
                }
            }
        }
        if let Some(event) = backfill.pending.pop() {
            let samples = crate::encode::read_ogg_opus_16k(&event.audio_path).ok();
            let reply = samples.as_deref().and_then(|s| self.separate_samples(shared, s));
            if reply.is_none() && samples.is_some() && self.worker.as_ref().is_none_or(Worker::given_up) {
                // The worker is gone for the session: leave the conversation untouched.
                set_backfill(shared, None);
                return;
            }
            if shared.cancel_asr.load(Ordering::Acquire) || !shared.settings.lock().speakers_enabled {
                return; // shutting down or switched off: nothing of this conversation is saved
            }
            let outcome = match reply {
                Some(reply) => {
                    let spans = spans(&mut backfill.tracker, &reply);
                    let voices = event
                        .segments
                        .iter()
                        .map(|&(id, start, end)| (id, assign::majority(start, end, &spans, assign::WHOLE_SEGMENT_SHARE)))
                        .collect();
                    (event.id, voices, SpeakersState::Done)
                }
                None => (event.id, Vec::new(), SpeakersState::Failed),
            };
            backfill.results.push(outcome);
        }
        if backfill.pending.is_empty() && !backfill.results.is_empty() {
            commit_backfill(shared, &backfill.tracker, std::mem::take(&mut backfill.results));
        }
        self.backfill = Some(backfill);
    }

    pub fn shutdown(&mut self) {
        self.worker = None;
        self.live.reset();
        self.backfill = None;
    }
}

/// Save one backfilled conversation: voices with enough speech get "Speaker N" rows (numbered
/// by first appearance), their segments are labelled, every event gets its outcome; one
/// transaction, under the persist gate, announced under the gate.
fn commit_backfill(shared: &Shared, tracker: &Tracker, results: Vec<EventResult>) {
    let mut speakers = Vec::new();
    let mut ids: HashMap<VoiceId, Uuid> = HashMap::new();
    for (voice, number) in tracker.due() {
        let id = Uuid::now_v7();
        ids.insert(voice, id);
        speakers.push((id, numbered_label(number), number));
    }
    let segments: Vec<(Uuid, Uuid)> = results
        .iter()
        .flat_map(|(_, voices, _)| voices.iter().filter_map(|&(segment, voice)| Some((segment, *ids.get(&voice?)?))))
        .collect();
    let states: Vec<(Uuid, SpeakersState)> = results.iter().map(|(event, _, state)| (*event, *state)).collect();
    let _gate = shared.persist_gate.lock();
    match shared.store.save_backfill(&speakers, &segments, &states) {
        Ok(events) => announce(shared, events),
        Err(_) => shared.notice("could not save separated speakers"),
    }
}

/// Map the worker's turns to conversation voices through the tracker.
fn spans(tracker: &mut Tracker, reply: &Reply) -> Vec<Span> {
    let voices = tracker.assign(&reply.voices);
    let by_local: HashMap<u32, Option<VoiceId>> =
        reply.voices.iter().zip(voices).map(|(v, id)| (v.local, id)).collect();
    reply
        .turns
        .iter()
        .map(|t| Span { start_ms: t.start_ms, end_ms: t.end_ms, voice: by_local.get(&t.local).copied().flatten() })
        .collect()
}

/// Create speaker rows for voices that just reached the threshold: (voice, number, id).
fn create_due(shared: &Shared, tracker: &Tracker) -> Vec<(VoiceId, u32, Uuid)> {
    tracker
        .due()
        .into_iter()
        .filter_map(|(voice, number)| {
            shared.store.create_person_speaker(&numbered_label(number), number).ok().map(|s| (voice, number, s.id))
        })
        .collect()
}

fn drop_created(shared: &Shared, created: &[(VoiceId, u32, Uuid)]) {
    let ids: Vec<Uuid> = created.iter().map(|c| c.2).collect();
    if !ids.is_empty() && shared.store.delete_unused_speakers(&ids).is_err() {
        tracing::warn!("could not remove unused speaker rows");
    }
}

/// Record the rows `create_due` made and relabel the segments each voice collected before it
/// was shown. Returns the events whose segments changed.
fn promote(shared: &Shared, tracker: &mut Tracker, created: &[(VoiceId, u32, Uuid)]) -> Vec<Uuid> {
    let mut relabel = Vec::new();
    for &(voice, number, speaker) in created {
        relabel.extend(tracker.promoted(voice, number, speaker).into_iter().map(|(_, segment)| (segment, Some(speaker))));
    }
    if relabel.is_empty() {
        return Vec::new();
    }
    shared.store.set_segment_speakers(&relabel).unwrap_or_else(|_| {
        shared.notice("could not label earlier speech with its speaker");
        Vec::new()
    })
}

/// Re-announce changed events. Call with the persist gate held, so an announcement can't be
/// read before a deletion and delivered after it.
pub(super) fn announce(shared: &Shared, events: Vec<Uuid>) {
    for id in events {
        if shared.upsert(id).is_err() {
            shared.notice("could not announce relabelled speech");
            break;
        }
    }
}

fn set_state(shared: &Shared, state: SeparationState) {
    let changed = {
        let mut status = shared.status.lock();
        std::mem::replace(&mut status.separation.state, state) != state
    };
    if changed {
        shared.publish();
    }
}

fn set_backfill(shared: &Shared, remaining: Option<u32>) {
    let changed = {
        let mut status = shared.status.lock();
        std::mem::replace(&mut status.separation.backfill_remaining, remaining) != remaining
    };
    if changed {
        shared.publish();
    }
}
