use std::fs;
use std::path::PathBuf;

use tuldmi_engine::storage::*;
use tuldmi_engine::types::*;

fn store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    (dir, s)
}

fn audio_file(dir: &std::path::Path, name: &str) -> NewAudio {
    let audio_dir = dir.join("audio");
    fs::create_dir_all(&audio_dir).unwrap();
    let path = audio_dir.join(name);
    fs::write(&path, b"OggS fake").unwrap();
    NewAudio { path, sample_rate: 16_000, duration_ms: 2_000, bytes: 9, peaks: vec![10, 200, 30] }
}

fn event(s: &Store, dir: &std::path::Path, session: &Session, source: Source, at: i64) -> SpeechEvent {
    s.insert_event(&NewEvent {
        session_id: session.id,
        source,
        started_at: at,
        ended_at: at + 2_000,
        audio: Some(audio_file(dir, &format!("{at}-{}.ogg", source.as_str()))),
    })
    .unwrap()
}

fn transcribe(s: &Store, id: uuid::Uuid, text: &str) -> SpeechEvent {
    s.save_transcript(
        id,
        &[NewSegment { start_ms: 0, end_ms: 1_500, text: text.into(), confidence: Some(0.9) }],
        Some("en"),
        "whisper-base.en",
    )
    .unwrap()
}

#[test]
fn migrations_apply_once_and_reopen_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    {
        let s = Store::open(dir.path()).unwrap();
        assert_eq!(s.schema_version().unwrap(), LATEST_SCHEMA);
        assert_eq!(s.default_speaker(Source::Mic).unwrap().kind, "self");
        assert_eq!(s.default_speaker(Source::System).unwrap().kind, "remote");
    }
    let s = Store::open(dir.path()).unwrap();
    assert_eq!(s.schema_version().unwrap(), LATEST_SCHEMA);
    let me1 = s.default_speaker(Source::Mic).unwrap();
    let me2 = s.default_speaker(Source::Mic).unwrap();
    assert_eq!(me1.id, me2.id, "speakers must not be re-seeded");
    assert_eq!(me1.label, "Me");
    assert_eq!(s.default_speaker(Source::System).unwrap().label, "Remote");
}

#[test]
fn event_lifecycle_and_listing() {
    let (dir, s) = store();
    let sess = s.start_session(1_000).unwrap();
    let a = event(&s, dir.path(), &sess, Source::Mic, 10_000);
    let b = event(&s, dir.path(), &sess, Source::System, 20_000);
    let c = event(&s, dir.path(), &sess, Source::Mic, 30_000);
    assert_eq!(a.status, EventStatus::Queued);
    assert_eq!(a.speaker.as_ref().unwrap().label, "Me");
    assert_eq!(b.speaker.as_ref().unwrap().label, "Remote");
    let audio = a.audio.as_ref().unwrap();
    assert!(PathBuf::from(&audio.path).is_absolute());
    assert_eq!(audio.peaks, vec![10, 200, 30]);
    assert!(!a.audio_expired);
    assert_eq!(s.backlog().unwrap(), 3);

    let all = s.list_events(None, 10).unwrap();
    assert_eq!(all.iter().map(|e| e.id).collect::<Vec<_>>(), vec![c.id, b.id, a.id], "newest first");
    let page = s.list_events(Some(30_000), 1).unwrap();
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, b.id);

    let done = transcribe(&s, a.id, "Let's move the budget review to Thursday.");
    assert_eq!(done.status, EventStatus::Done);
    assert_eq!(done.segments.len(), 1);
    assert_eq!(done.model_id.as_deref(), Some("whisper-base.en"));
    assert_eq!(s.get_event(a.id).unwrap().unwrap().segments[0].text, "Let's move the budget review to Thursday.");

    let empty = s.save_transcript(b.id, &[NewSegment { start_ms: 0, end_ms: 10, text: "   ".into(), confidence: None }], None, "m").unwrap();
    assert_eq!(empty.status, EventStatus::Empty);

    // Re-saving replaces, never duplicates.
    let again = transcribe(&s, a.id, "Replaced text.");
    assert_eq!(again.segments.len(), 1);
    assert_eq!(again.segments[0].text, "Replaced text.");

    let path = s.delete_event(c.id).unwrap().unwrap();
    assert!(path.ends_with("30000-mic.ogg"));
    assert!(s.get_event(c.id).unwrap().is_none());
    assert_eq!(s.counts().unwrap(), (2, 1));
}

#[test]
fn job_queue_claims_oldest_and_retries_then_fails() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let a = event(&s, dir.path(), &sess, Source::Mic, 5_000);
    let b = event(&s, dir.path(), &sess, Source::Mic, 6_000);

    let j = s.next_job().unwrap().unwrap();
    assert_eq!(j.event_id, a.id);
    assert_eq!(j.attempts, 1);
    assert!(j.audio_path.unwrap().exists());
    assert_eq!(s.get_event(a.id).unwrap().unwrap().status, EventStatus::Transcribing);

    // Retryable failures requeue until MAX_ATTEMPTS.
    s.fail_job(a.id, "decoder hiccup", true).unwrap();
    assert_eq!(s.get_event(a.id).unwrap().unwrap().status, EventStatus::Queued);
    for attempt in 2..=MAX_ATTEMPTS {
        let j = s.next_job().unwrap().unwrap();
        assert_eq!(j.event_id, a.id, "requeued job keeps its place at the front");
        assert_eq!(j.attempts, attempt);
        s.fail_job(a.id, "decoder hiccup", true).unwrap();
    }
    let failed = s.get_event(a.id).unwrap().unwrap();
    assert_eq!(failed.status, EventStatus::Failed);
    assert_eq!(failed.error.as_deref(), Some("decoder hiccup"));

    let j = s.next_job().unwrap().unwrap();
    assert_eq!(j.event_id, b.id);
    s.fail_job(b.id, "model missing", false).unwrap();
    assert_eq!(s.get_event(b.id).unwrap().unwrap().status, EventStatus::Failed);
    assert!(s.next_job().unwrap().is_none());
    assert_eq!(s.backlog().unwrap(), 0);

    assert!(s.retry_event(a.id).unwrap());
    let j = s.next_job().unwrap().unwrap();
    assert_eq!(j.event_id, a.id);
    assert_eq!(j.attempts, 1, "user retry resets attempts");
}

#[test]
fn crash_recovery_requeues_running_jobs_and_closes_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let (sess_id, ev_id) = {
        let s = Store::open(dir.path()).unwrap();
        let sess = s.start_session(100).unwrap();
        let e = event(&s, dir.path(), &sess, Source::Mic, 1_000);
        s.next_job().unwrap().unwrap(); // running when the "crash" happens
        (sess.id, e.id)
    };
    let s = Store::open(dir.path()).unwrap();
    assert_eq!(s.recover_interrupted_jobs().unwrap(), 1);
    assert_eq!(s.get_event(ev_id).unwrap().unwrap().status, EventStatus::Queued);
    assert_eq!(s.backlog().unwrap(), 1);
    assert_eq!(s.close_dangling_sessions().unwrap(), 1);
    assert_eq!(s.close_dangling_sessions().unwrap(), 0);
    let _ = sess_id;
}

#[test]
fn search_matches_highlights_filters_and_survives_hostile_input() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let a = event(&s, dir.path(), &sess, Source::Mic, 1_000_000);
    let b = event(&s, dir.path(), &sess, Source::System, 2_000_000);
    let c = event(&s, dir.path(), &sess, Source::Mic, 3_000_000);
    transcribe(&s, a.id, "The budget review moves to Thursday afternoon.");
    transcribe(&s, b.id, "Can you share the budget spreadsheet before the call?");
    transcribe(&s, c.id, "Café plans for the weekend.");

    let q = |text: &str| SearchQuery { text: text.into(), ..Default::default() };
    let hits = s.search(&q("budget")).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].snippet.contains(&format!("{HIGHLIGHT_START}budget{HIGHLIGHT_END}")) || hits[0].snippet.contains(&format!("{HIGHLIGHT_START}Budget{HIGHLIGHT_END}")));
    assert_eq!(hits.iter().find(|h| h.event_id == a.id).unwrap().at, 1_000_000);

    // Prefix on the last token, AND across tokens.
    assert_eq!(s.search(&q("budg")).unwrap().len(), 2);
    assert_eq!(s.search(&q("budget thurs")).unwrap().len(), 1);
    // Diacritics folded.
    assert_eq!(s.search(&q("cafe")).unwrap().len(), 1);

    // Filters.
    let sys_only = s.search(&SearchQuery { text: "budget".into(), sources: vec![Source::System], ..Default::default() }).unwrap();
    assert_eq!(sys_only.len(), 1);
    assert_eq!(sys_only[0].source, Source::System);
    let ranged = s.search(&SearchQuery { text: "budget".into(), from: Some(1_500_000), to: Some(2_500_000), ..Default::default() }).unwrap();
    assert_eq!(ranged.len(), 1);
    assert_eq!(ranged[0].event_id, b.id);

    // Hostile input never errors.
    for nasty in ["\"", "budget\"", "NOT", "a OR b", "-budget", "budget*", "(", "^", "NEAR(a b)", "col:val", "'; DROP TABLE x;--", "    ", ""] {
        let r = s.search(&q(nasty));
        assert!(r.is_ok(), "query {nasty:?} errored: {:?}", r.err());
    }
    assert!(s.search(&q("   ")).unwrap().is_empty());

    // Deleted events disappear from the index.
    s.delete_event(a.id).unwrap();
    assert_eq!(s.search(&q("budget")).unwrap().len(), 1);
}

#[test]
fn retention_expires_only_finished_audio_and_keeps_transcripts() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let old_done = event(&s, dir.path(), &sess, Source::Mic, 1_000);
    let old_queued = event(&s, dir.path(), &sess, Source::Mic, 2_000);
    let new_done = event(&s, dir.path(), &sess, Source::Mic, 100_000);
    transcribe(&s, old_done.id, "old words");
    transcribe(&s, new_done.id, "new words");

    let paths = s.expire_audio_before(50_000).unwrap();
    assert_eq!(paths.len(), 1);
    assert!(paths[0].ends_with("1000-mic.ogg"));

    let e = s.get_event(old_done.id).unwrap().unwrap();
    assert!(e.audio.is_none());
    assert!(e.audio_expired);
    assert_eq!(e.segments[0].text, "old words");
    assert!(s.get_event(old_queued.id).unwrap().unwrap().audio.is_some(), "untranscribed audio must survive retention");
    assert!(s.get_event(new_done.id).unwrap().unwrap().audio.is_some());
    assert_eq!(s.search(&SearchQuery { text: "old".into(), ..Default::default() }).unwrap().len(), 1);

    // Expiring again is a no-op.
    assert!(s.expire_audio_before(50_000).unwrap().is_empty());

    let p = s.expire_event_audio(new_done.id).unwrap().unwrap();
    assert!(p.ends_with("100000-mic.ogg"));
    let (bytes, files) = s.audio_usage().unwrap();
    assert_eq!(files, 1);
    assert_eq!(bytes, 9);

    // A file nobody references is an orphan.
    for p in paths.iter().chain([&p]) {
        fs::remove_file(p).unwrap();
    }
    fs::write(dir.path().join("audio").join("stray.ogg"), b"x").unwrap();
    let orphans = s.orphan_audio_files().unwrap();
    assert_eq!(orphans.len(), 1);
    assert!(orphans[0].ends_with("stray.ogg"));

    let rest = s.delete_all_history().unwrap();
    assert_eq!(rest.len(), 1);
    assert_eq!(s.counts().unwrap(), (0, 0));
}

#[test]
fn settings_roundtrip_and_defaults() {
    let (_dir, s) = store();
    let d = s.load_settings().unwrap();
    assert_eq!(d, Settings::default());
    let mut changed = d.clone();
    changed.retention = Retention::Days7;
    changed.mic_device_id = Some("{0.0.1.00000000}.{abc}".into());
    changed.vad.threshold = 0.62;
    changed.onboarding_complete = true;
    s.save_settings(&changed).unwrap();
    assert_eq!(s.load_settings().unwrap(), changed);
}

#[test]
fn models_are_recorded() {
    let (dir, s) = store();
    let m = InstalledModel { model_id: "whisper-base.en".into(), path: dir.path().join("models").join("ggml-base.en.bin"), bytes: 147_951_465, sha256: "abc".into() };
    s.record_model(&m).unwrap();
    s.record_model(&m).unwrap(); // upsert
    assert_eq!(s.installed_models().unwrap(), vec![m.clone()]);
    s.remove_model("whisper-base.en").unwrap();
    assert!(s.installed_models().unwrap().is_empty());
}

// --- regressions from the 2026-10-24 adversarial review (docs/dev-process.md, unit 1) ---

#[test]
fn recovery_caps_attempts_so_a_crashing_job_cannot_loop_forever() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let e = event(&s, dir.path(), &sess, Source::Mic, 1_000);
    for _ in 0..MAX_ATTEMPTS {
        assert!(s.next_job().unwrap().is_some());
        s.recover_interrupted_jobs().unwrap(); // the app "crashed" mid-transcription
    }
    let ev = s.get_event(e.id).unwrap().unwrap();
    assert_eq!(ev.status, EventStatus::Failed, "a job interrupted MAX_ATTEMPTS times must stop being retried automatically");
    assert!(ev.error.unwrap_or_default().to_lowercase().contains("interrupted"));
    assert!(s.next_job().unwrap().is_none());
    assert!(s.retry_event(e.id).unwrap(), "the user can still retry it");
}

#[test]
fn retention_never_expires_audio_without_a_successful_transcript() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let failed = event(&s, dir.path(), &sess, Source::Mic, 1_000);
    s.next_job().unwrap().unwrap();
    s.fail_job(failed.id, "decoder error", false).unwrap();
    assert_eq!(s.get_event(failed.id).unwrap().unwrap().status, EventStatus::Failed);
    assert!(s.expire_audio_before(i64::MAX).unwrap().is_empty(), "failed events keep their audio so they can be retried");
    assert!(s.retry_event(failed.id).unwrap());
}

#[test]
fn search_handles_punctuation_inside_words() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let a = event(&s, dir.path(), &sess, Source::Mic, 1_000);
    transcribe(&s, a.id, "Let's do a follow-up on the e-mail about version 3.5 in the U.S. office.");
    for q in ["follow-up", "e-mail", "3.5", "U.S.", "let's", "Let’s", "follow-up e-mail", "version 3.5"] {
        let hits = s.search(&SearchQuery { text: q.into(), ..Default::default() }).unwrap();
        assert_eq!(hits.len(), 1, "query {q:?} should match");
    }
}

#[test]
fn paging_with_a_cursor_does_not_skip_equal_timestamps() {
    let (dir, s) = store();
    let sess = s.start_session(0).unwrap();
    let a = s.insert_event(&NewEvent { session_id: sess.id, source: Source::Mic, started_at: 5_000, ended_at: 6_000, audio: Some(audio_file(dir.path(), "a.ogg")) }).unwrap();
    let b = s.insert_event(&NewEvent { session_id: sess.id, source: Source::System, started_at: 5_000, ended_at: 6_000, audio: Some(audio_file(dir.path(), "b.ogg")) }).unwrap();
    let first = s.list_events_page(None, 1).unwrap();
    assert_eq!(first.len(), 1);
    let cursor = (first[0].started_at, first[0].id);
    let second = s.list_events_page(Some(cursor), 1).unwrap();
    assert_eq!(second.len(), 1);
    let mut ids = vec![first[0].id, second[0].id];
    ids.sort();
    let mut want = vec![a.id, b.id];
    want.sort();
    assert_eq!(ids, want);
    assert!(s.list_events_page(Some((second[0].started_at, second[0].id)), 1).unwrap().is_empty());
}

#[test]
fn deleted_transcripts_do_not_linger_in_database_files() {
    let dir = tempfile::tempdir().unwrap();
    let phrase = "zanzibar-quokka-secret";
    {
        let s = Store::open(dir.path()).unwrap();
        let sess = s.start_session(0).unwrap();
        let e = event(&s, dir.path(), &sess, Source::Mic, 1_000);
        transcribe(&s, e.id, &format!("the password is {phrase} okay"));
        s.delete_all_history().unwrap();
    }
    for name in ["tuldmi.db", "tuldmi.db-wal"] {
        let p = dir.path().join(name);
        if let Ok(bytes) = fs::read(&p) {
            assert!(!bytes.windows(phrase.len()).any(|w| w == phrase.as_bytes()), "deleted text still present in {name}");
        }
    }
}
