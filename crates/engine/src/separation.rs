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
    protocol::{Kind, Reply},
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
    /// The settings generation the voices held here belong to.
    generation: u64,
    /// (speakers, music) switches last reconciled with the worker while idle.
    switches: Option<(bool, bool)>,
}

/// One backfilled event: its segments' voices and its outcome.
type EventResult = (Uuid, i64, Vec<(Uuid, Option<VoiceId>)>, SpeakersState);

struct Backfill {
    /// Start of the last event of the conversation being worked on (resume point).
    cursor: i64,
    /// Remaining events of that conversation, last first.
    pending: Vec<BackfillEvent>,
    tracker: Tracker,
    /// Settings generation this backfill started in.
    generation: u64,
    /// Per event already processed: its segments' voices and its outcome. Saved together.
    results: Vec<EventResult>,
    conversations: u32,
}

/// What a live separation decided, to be saved under the persist gate.
#[cfg_attr(not(feature = "whisper"), allow(dead_code))]
pub(super) struct Separated {
    labelled: Vec<Labelled>,
    state: SpeakersState,
    /// Settings generation the voices came from; re-checked under the persist gate.
    generation: u64,
}

impl Separation {
    pub fn new() -> Self {
        Separation { worker: None, live: Tracker::default(), number_from: None, backfill: None, gave_up_noticed: false, generation: 0, switches: None }
    }

    /// Call on every loop of the transcription thread. Switching the feature off forgets every
    /// voice and stops the worker and any backfill at once, not at the next system event. While
    /// idle (`idle`: nothing waits for transcription), a conversation that can no longer
    /// continue is ended, dropping its voices.
    pub fn housekeep(&mut self, shared: &Shared, idle: bool) {
        let generation = shared.speakers_generation.load(Ordering::Acquire);
        if generation != self.generation {
            // Switched off (and maybe on again) since these voices were heard: forget them.
            self.generation = generation;
            self.live.reset();
            self.number_from = None;
            if self.backfill.take().is_some() {
                set_backfill(shared, None);
            }
        }
        let switches = {
            let settings = shared.settings.lock();
            (settings.speakers_enabled, settings.music_rejection)
        };
        if idle && self.switches != Some(switches) {
            // A feature switched while idle: start, reconfigure or stop the worker now, not at
            // the next recording (a worker nothing needs any more doesn't linger).
            self.switches = Some(switches);
            let state = self.check(shared);
            set_state(shared, state);
        }
        if !shared.settings.lock().speakers_enabled {
            // The worker itself stays if music rejection still needs it; `check` reconfigures it.
            self.live.reset();
            self.number_from = None;
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
            if self.backfill.take().is_some() | shared.backfill_requested.swap(false, Ordering::AcqRel) {
                set_backfill(shared, None);
            }
        }
        set_state(shared, state);
        state == SeparationState::Ready
    }

    /// Keep the worker configured for what's wanted now: speaker separation (switched on and
    /// both models installed) and music rejection (switched on and the bundled classifier
    /// present). A change restarts it; with neither, there's no worker. Returns the speaker
    /// separation state.
    fn check(&mut self, shared: &Shared) -> SeparationState {
        let (speakers_on, music_on) = {
            let settings = shared.settings.lock();
            (settings.speakers_enabled, settings.music_rejection)
        };
        // Whatever path reconciles the worker, housekeeping now knows which switches it reflects.
        self.switches = Some((speakers_on, music_on));
        let program = shared.config.speaker_worker.clone().filter(|p| p.is_file());
        let speakers = if speakers_on {
            shared.store.installed_models().ok().and_then(|installed| {
                let paths: Option<Vec<_>> = SPEAKER_CATALOG.iter().map(|e| installed_path(e, &installed)).collect();
                paths.and_then(|p| <[_; 2]>::try_from(p).ok()).map(|[segmentation, embedding]| (segmentation, embedding))
            })
        } else {
            None
        };
        let sound = shared.config.sound_model.clone().filter(|(model, labels)| music_on && model.is_file() && labels.is_file());
        let mut state = match (speakers_on, &program, &speakers) {
            (false, _, _) => SeparationState::Off,
            (true, None, _) => SeparationState::Unavailable,
            (true, Some(_), None) => SeparationState::NeedsModels,
            (true, Some(_), Some(_)) => SeparationState::Ready,
        };
        match program {
            Some(program) if speakers.is_some() || sound.is_some() => {
                let config = WorkerConfig { program, speakers, sound };
                if self.worker.as_ref().map(Worker::config) != Some(&config) {
                    let mut worker = Worker::new(config);
                    worker.set_cancel(shared.cancel_asr.clone());
                    self.worker = Some(worker);
                    self.gave_up_noticed = false;
                }
            }
            _ => self.worker = None,
        }
        if self.worker.as_ref().is_some_and(|w| w.given_up_on(Kind::Separate)) && state == SeparationState::Ready {
            if !std::mem::replace(&mut self.gave_up_noticed, true) {
                shared.notice("speaker separation stopped after repeated failures; transcripts are unaffected. Restart tuldMi to try again");
            }
            state = SeparationState::Unavailable;
        }
        state
    }

    /// Score an event's audio for music rejection. None = don't judge it (switched off, no
    /// classifier, too short to tell, or the worker failed): it is transcribed as before.
    #[cfg_attr(not(feature = "whisper"), allow(dead_code))]
    pub fn classify(&mut self, shared: &Shared, samples: &[f32]) -> Option<crate::speakers::protocol::Sound> {
        // Under a second, the classifier has too little to go on; such clips are short words.
        if samples.len() < 16_000 {
            return None;
        }
        set_state(shared, self.check(shared));
        let worker = self.worker.as_mut()?;
        if worker.config().sound.is_none() || worker.given_up_on(Kind::Classify) {
            return None;
        }
        worker.classify(samples).ok()
    }

    fn separate_samples(&mut self, shared: &Shared, samples: &[f32]) -> Option<Reply> {
        let worker = self.worker.as_mut()?;
        set_state(shared, SeparationState::Working);
        let reply = worker.separate(samples).ok();
        set_state(shared, if worker.given_up_on(Kind::Separate) { SeparationState::Unavailable } else { SeparationState::Ready });
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
        if self.number_from.take().is_some() {
            // After a restart mid-conversation the voices are gone (never stored): continue
            // numbering past every number this conversation already used, before or after
            // this event (it may be a retry).
            let used = shared
                .store
                .conversation_bounds(job.started_at)
                .and_then(|(first, last)| shared.store.conversation_max_ordinal(first, last))
                .unwrap_or(0);
            self.live.set_numbered(used);
        }
        let generation = shared.speakers_generation.load(Ordering::Acquire);
        let reply = self.separate_samples(shared, samples);
        // Switched off (even if back on) while the worker ran: don't label with voices from
        // before the switch. `housekeep` forgets them on the next loop.
        if shared.speakers_generation.load(Ordering::Acquire) != generation || generation != self.generation {
            self.live.reset();
            return None;
        }
        let Some(reply) = reply else {
            return Some(Separated {
                labelled: transcript.segments.iter().map(|s| Labelled { segment: s.clone(), voice: None }).collect(),
                state: SpeakersState::Failed,
                generation,
            });
        };
        let spans = spans(&mut self.live, &reply);
        Some(Separated { labelled: assign::label(&transcript.segments, &transcript.pieces, &spans), state: SpeakersState::Done, generation })
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
        // Under the persist gate now: separation switched off (or off and on) since the worker
        // answered means these voices were dismissed. Save the words only, never separated.
        if shared.speakers_generation.load(Ordering::Acquire) != separated.generation || !shared.settings.lock().speakers_enabled {
            self.live.reset();
            let plain: Vec<NewSegment> =
                separated.labelled.into_iter().map(|l| NewSegment { speaker_id: None, ..l.segment }).collect();
            return shared.store.save_transcript(job.event_id, &plain, language, model_id).map(|event| (event, Vec::new()));
        }
        let created = create_due(shared, &self.live, job.started_at);
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
                generation: shared.speakers_generation.load(Ordering::Acquire),
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
            let generation = shared.speakers_generation.load(Ordering::Acquire);
            let samples = crate::encode::read_ogg_opus_16k(&event.audio_path).ok();
            let reply = samples.as_deref().and_then(|s| self.separate_samples(shared, s));
            if reply.is_none() && samples.is_some() && self.worker.as_ref().is_none_or(|w| w.given_up_on(Kind::Separate)) {
                // The worker is gone for the session: leave the conversation untouched.
                set_backfill(shared, None);
                return;
            }
            if shared.cancel_asr.load(Ordering::Acquire)
                || shared.speakers_generation.load(Ordering::Acquire) != generation
                || generation != self.generation
            {
                return; // shutting down or switched off/on: nothing of this conversation is saved
            }
            let outcome = match reply {
                Some(reply) => {
                    let spans = spans(&mut backfill.tracker, &reply);
                    let voices = event
                        .segments
                        .iter()
                        .map(|&(id, start, end)| (id, assign::majority(start, end, &spans, assign::WHOLE_SEGMENT_SHARE)))
                        .collect();
                    (event.id, event.started_at, voices, SpeakersState::Done)
                }
                None => (event.id, event.started_at, Vec::new(), SpeakersState::Failed),
            };
            backfill.results.push(outcome);
        }
        if backfill.pending.is_empty() && !backfill.results.is_empty() {
            commit_backfill(shared, &backfill.tracker, generation_at_start(&backfill), std::mem::take(&mut backfill.results));
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
fn commit_backfill(shared: &Shared, tracker: &Tracker, generation: u64, results: Vec<EventResult>) {
    let mut speakers = Vec::new();
    let mut ids: HashMap<VoiceId, Uuid> = HashMap::new();
    for (voice, number) in tracker.due() {
        // First shown in the earliest event that has a segment of this voice.
        let seen_at = results
            .iter()
            .find(|(_, _, voices, _)| voices.iter().any(|&(_, v)| v == Some(voice)))
            .map_or(0, |(_, started, _, _)| *started);
        let id = Uuid::now_v7();
        ids.insert(voice, id);
        speakers.push((id, numbered_label(number), number, seen_at));
    }
    let segments: Vec<(Uuid, Uuid)> = results
        .iter()
        .flat_map(|(_, _, voices, _)| voices.iter().filter_map(|&(segment, voice)| Some((segment, *ids.get(&voice?)?))))
        .collect();
    let states: Vec<(Uuid, SpeakersState)> = results.iter().map(|(event, _, _, state)| (*event, *state)).collect();
    let _gate = shared.persist_gate.lock();
    // Switched off (or off and on) while this conversation was processed: save nothing.
    if shared.speakers_generation.load(Ordering::Acquire) != generation || !shared.settings.lock().speakers_enabled {
        return;
    }
    match shared.store.save_backfill(&speakers, &segments, &states) {
        Ok(events) => announce(shared, events),
        Err(_) => shared.notice("could not save separated speakers"),
    }
}

fn generation_at_start(backfill: &Backfill) -> u64 {
    backfill.generation
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
fn create_due(shared: &Shared, tracker: &Tracker, seen_at: i64) -> Vec<(VoiceId, u32, Uuid)> {
    tracker
        .due()
        .into_iter()
        .filter_map(|(voice, number)| {
            shared.store.create_person_speaker(&numbered_label(number), number, seen_at).ok().map(|s| (voice, number, s.id))
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
