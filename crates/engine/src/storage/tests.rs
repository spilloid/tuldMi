//! Additional checks for requirements beyond the integration contract.

use super::*;

fn setup() -> Result<(tempfile::TempDir, Store)> {
    let directory = tempfile::tempdir()?;
    let store = Store::open(directory.path())?;
    Ok((directory, store))
}

fn audio(path: PathBuf) -> NewAudio {
    NewAudio {
        path,
        sample_rate: 16_000,
        duration_ms: 100,
        bytes: 4,
        peaks: vec![1, 2],
    }
}

fn insert(
    store: &Store,
    session: &Session,
    at: i64,
    asset: Option<NewAudio>,
) -> Result<SpeechEvent> {
    store.insert_event(&NewEvent {
        session_id: session.id,
        source: Source::Mic,
        started_at: at,
        ended_at: at + 100,
        audio: asset,
    })
}

#[test]
fn connection_settings_paths_and_seed_ids_survive_reopen() -> Result<()> {
    let (directory, store) = setup()?;
    let speaker = store.default_speaker(Source::Mic)?;
    let session = store.start_session(0)?;
    let event = insert(
        &store,
        &session,
        1,
        Some(audio(directory.path().join("audio/nested/a.ogg"))),
    )?;
    assert_eq!(event.id.get_version_num(), 7);
    assert_eq!(speaker.id.get_version_num(), 7);
    {
        let connection = store.connection.lock();
        for (pragma, expected) in [
            // FULL: a committed event row must not roll back after power loss while its
            // audio file survives (review R1 finding 7).
            ("synchronous", 2),
            ("foreign_keys", 1),
            ("busy_timeout", 5000),
        ] {
            assert_eq!(
                connection.pragma_query_value(None, pragma, |row| row.get::<_, i64>(0))?,
                expected
            );
        }
        assert_eq!(
            connection.pragma_query_value(None, "journal_mode", |row| row.get::<_, String>(0))?,
            "wal"
        );
        let (id, path): (String, String) =
            connection.query_row("SELECT id, rel_path FROM audio_assets", [], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })?;
        assert_eq!(
            id,
            Uuid::parse_str(&id)
                .map_err(|e| Error::Other(e.to_string()))?
                .to_string()
        );
        assert_eq!(path, "audio/nested/a.ogg");
    }
    drop(store);
    let reopened = Store::open(directory.path())?;
    assert_eq!(reopened.default_speaker(Source::Mic)?, speaker);
    assert_eq!(reopened.get_event(event.id)?, Some(event));
    Ok(())
}

#[test]
fn outside_paths_and_failed_event_insert_leave_no_rows() -> Result<()> {
    let (directory, store) = setup()?;
    let outside = tempfile::tempdir()?;
    let session = store.start_session(0)?;
    for path in [
        outside.path().join("outside.ogg"),
        PathBuf::from("audio/relative.ogg"),
        directory.path().join("../outside.ogg"),
    ] {
        assert!(insert(&store, &session, 1, Some(audio(path))).is_err());
    }
    let invalid_session = Session {
        id: Uuid::now_v7(),
        started_at: 0,
        ended_at: None,
    };
    assert!(
        insert(
            &store,
            &invalid_session,
            1,
            Some(audio(directory.path().join("audio/a.ogg")))
        )
        .is_err()
    );
    assert_eq!(store.counts()?, (0, 0));
    assert_eq!(store.audio_usage()?, (0, 0));
    assert_eq!(store.backlog()?, 0);
    Ok(())
}

#[test]
fn transcript_failure_rolls_back_segments_fts_and_status() -> Result<()> {
    let (_directory, store) = setup()?;
    let session = store.start_session(0)?;
    let event = insert(&store, &session, 10, None)?;
    let segment = NewSegment {
        start_ms: 7,
        end_ms: 20,
        text: "original words".into(),
        confidence: None,
        speaker_id: None,
    };
    let original = store.save_transcript(event.id, &[segment], Some("en"), "model")?;
    store.connection.lock().execute_batch(
        "CREATE TEMP TRIGGER reject_segment BEFORE INSERT ON transcript_segments
         WHEN new.text = 'reject' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )?;
    let segments = [
        NewSegment {
            start_ms: 0,
            end_ms: 10,
            text: "replacement".into(),
            confidence: None,
            speaker_id: None,
        },
        NewSegment {
            start_ms: 10,
            end_ms: 20,
            text: "reject".into(),
            confidence: None,
            speaker_id: None,
        },
    ];
    assert!(
        store
            .save_transcript(event.id, &segments, None, "other")
            .is_err()
    );
    assert_eq!(store.get_event(event.id)?, Some(original));
    assert_eq!(
        store
            .search(&SearchQuery {
                text: "original".into(),
                ..Default::default()
            })?
            .len(),
        1
    );
    assert!(
        store
            .search(&SearchQuery {
                text: "replacement".into(),
                ..Default::default()
            })?
            .is_empty()
    );
    Ok(())
}

#[test]
fn settings_merge_and_invalid_values_fall_back() -> Result<()> {
    let (_directory, store) = setup()?;
    for (json, expected) in [
        (
            r#"{"micEnabled":false,"futureKey":42}"#,
            Settings {
                mic_enabled: false,
                ..Settings::default()
            },
        ),
        ("broken", Settings::default()),
        (r#"{"retention":"futurePolicy"}"#, Settings::default()),
    ] {
        store.connection.lock().execute(
            "INSERT INTO settings (key, value) VALUES ('settings', ?1)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [json],
        )?;
        assert_eq!(store.load_settings()?, expected);
    }
    Ok(())
}

#[test]
fn nested_orphans_and_expired_paths_are_found_without_unlinking() -> Result<()> {
    let (directory, store) = setup()?;
    assert!(store.orphan_audio_files()?.is_empty());
    std::fs::create_dir_all(directory.path().join("audio/nested/deeper"))?;
    let live = directory.path().join("audio/nested/live.ogg");
    let expired = directory.path().join("audio/nested/deeper/expired.ogg");
    let orphan = directory.path().join("audio/stray.ogg");
    for path in [&live, &expired, &orphan] {
        std::fs::write(path, b"OggS")?;
    }
    let session = store.start_session(0)?;
    insert(&store, &session, 1, Some(audio(live)))?;
    let event = insert(&store, &session, 2, Some(audio(expired.clone())))?;
    assert_eq!(store.expire_event_audio(event.id)?, Some(expired.clone()));
    assert_eq!(store.expire_event_audio(event.id)?, None);
    let mut expected = vec![expired, orphan];
    expected.sort();
    assert_eq!(store.orphan_audio_files()?, expected);
    assert!(expected.iter().all(|path| path.exists()));
    assert!(!store.retry_event(event.id)?);
    Ok(())
}

#[test]
fn search_sanitizes_tokens_and_obeys_event_dates_offsets_and_limits() -> Result<()> {
    let (_directory, store) = setup()?;
    let session = store.start_session(0)?;
    let mut last = None;
    for at in 0..205 {
        let event = insert(&store, &session, at, None)?;
        store.save_transcript(
            event.id,
            &[
                NewSegment {
                    start_ms: 70,
                    end_ms: 80,
                    text: "budget Thursday".into(),
                    confidence: None,
                    speaker_id: None,
                },
                NewSegment {
                    start_ms: 20,
                    end_ms: 30,
                    text: "other".into(),
                    confidence: None,
                    speaker_id: None,
                },
            ],
            None,
            "m",
        )?;
        last = Some(event.id);
    }
    let query = SearchQuery {
        text: "-budget (Thurs*)".into(),
        ..Default::default()
    };
    assert_eq!(store.search(&query)?.len(), DEFAULT_SEARCH_LIMIT as usize);
    let all = store.search(&SearchQuery {
        limit: Some(1000),
        ..query.clone()
    })?;
    assert_eq!(all.len(), 200);
    assert_eq!(Some(all[0].event_id), last);
    assert_eq!(all[0].at, 274);
    assert!(all[0].snippet.contains(HIGHLIGHT_START));
    let filtered = store.search(&SearchQuery {
        from: Some(204),
        to: Some(204),
        ..query.clone()
    })?;
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].at, 274);
    assert!(
        store
            .search(&SearchQuery {
                limit: Some(0),
                ..query
            })?
            .is_empty()
    );
    let page = store.list_events(None, 200)?;
    assert_eq!(page.len(), 200);
    assert!(
        page.iter()
            .all(|e| e.segments.len() == 2 && e.segments[0].start_ms == 20)
    );
    for text in ["'''", "\"*():^", "a\0b", "NEAR(a b)", "col:val"] {
        store.search(&SearchQuery {
            text: text.into(),
            ..Default::default()
        })?;
    }
    let filtered_only = store.search(&SearchQuery {
        from: Some(204),
        ..Default::default()
    })?;
    assert_eq!(filtered_only.len(), 2);
    Ok(())
}

#[test]
fn deletion_cascades_fts_and_preserves_settings_models_and_speakers() -> Result<()> {
    let (directory, store) = setup()?;
    let speaker = store.default_speaker(Source::Mic)?;
    let session = store.start_session(10)?;
    let event = insert(
        &store,
        &session,
        20,
        Some(audio(directory.path().join("audio/a.ogg"))),
    )?;
    store.save_transcript(
        event.id,
        &[NewSegment {
            start_ms: 0,
            end_ms: 10,
            text: "words".into(),
            confidence: None,
            speaker_id: None,
        }],
        None,
        "m",
    )?;
    let settings = Settings {
        mic_enabled: false,
        ..Settings::default()
    };
    store.save_settings(&settings)?;
    let model = InstalledModel {
        model_id: "m".into(),
        path: directory.path().join("models/m.bin"),
        bytes: 42,
        sha256: "abc".into(),
    };
    store.record_model(&model)?;
    assert_eq!(store.delete_all_history()?.len(), 1);
    assert_eq!(store.counts()?, (0, 0));
    assert_eq!(store.backlog()?, 0);
    assert_eq!(store.audio_usage()?, (0, 0));
    assert!(
        store
            .search(&SearchQuery {
                text: "words".into(),
                ..Default::default()
            })?
            .is_empty()
    );
    assert_eq!(store.load_settings()?, settings);
    assert_eq!(store.installed_models()?, vec![model]);
    assert_eq!(store.default_speaker(Source::Mic)?, speaker);
    assert_eq!(store.close_dangling_sessions()?, 0);
    Ok(())
}

#[test]
fn claiming_a_job_rolls_back_when_the_event_update_fails() -> Result<()> {
    let (_directory, store) = setup()?;
    let session = store.start_session(0)?;
    let event = insert(&store, &session, 1, None)?;
    store.connection.lock().execute_batch(
        "CREATE TEMP TRIGGER reject_claim BEFORE UPDATE ON audio_events
         WHEN new.status = 'transcribing' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
    )?;
    assert!(store.next_job().is_err());
    assert_eq!(store.backlog()?, 1);
    assert_eq!(
        store.get_event(event.id)?.map(|e| e.status),
        Some(EventStatus::Queued)
    );
    store
        .connection
        .lock()
        .execute_batch("DROP TRIGGER reject_claim")?;
    let job = store
        .next_job()?
        .ok_or_else(|| Error::Other("missing queued job".into()))?;
    assert_eq!(job.attempts, 1);
    assert_eq!(job.audio_path, None);
    assert_eq!(store.recover_interrupted_jobs()?, 1);
    let job = store
        .next_job()?
        .ok_or_else(|| Error::Other("missing recovered job".into()))?;
    assert_eq!(job.attempts, 2);
    Ok(())
}

#[test]
fn dangling_sessions_use_the_last_event_end_or_the_session_start() -> Result<()> {
    let (_directory, store) = setup()?;
    let empty = store.start_session(100)?;
    let occupied = store.start_session(200)?;
    let ended = store.start_session(300)?;
    insert(&store, &occupied, 500, None)?;
    insert(&store, &occupied, 1000, None)?;
    store.end_session(ended.id, 350)?;
    assert_eq!(store.close_dangling_sessions()?, 2);
    for (session, expected) in [(empty, 100), (occupied, 1100), (ended, 350)] {
        let actual: i64 = store.connection.lock().query_row(
            "SELECT ended_at FROM sessions WHERE id = ?1",
            [session.id.to_string()],
            |row| row.get(0),
        )?;
        assert_eq!(actual, expected);
    }
    assert_eq!(store.close_dangling_sessions()?, 0);
    Ok(())
}

#[test]
fn read_only_store_reads_filters_and_never_writes() -> Result<()> {
    let (directory, store) = setup()?;
    let session = store.start_session(0)?;
    let me = insert(&store, &session, 1_000, None)?;
    let remote = store.insert_event(&NewEvent {
        session_id: session.id,
        source: Source::System,
        started_at: 2_000,
        ended_at: 2_100,
        audio: None,
    })?;
    store.save_transcript(me.id, &[NewSegment { start_ms: 0, end_ms: 100, text: "send the deck".into(), confidence: None , speaker_id: None}], Some("en"), "tiny")?;
    store.save_transcript(remote.id, &[NewSegment { start_ms: 0, end_ms: 100, text: "the deck is late".into(), confidence: None , speaker_id: None}], Some("en"), "tiny")?;

    let reader = Store::open_read_only(directory.path())?;
    let all = |sources: Vec<Source>, from: Option<i64>| EventFilter { from, to: None, sources, newest_first: true, limit: 10 };
    let ids = |events: Vec<SpeechEvent>| events.into_iter().map(|e| e.id).collect::<Vec<_>>();
    assert_eq!(ids(reader.events_matching(&all(vec![], None))?), vec![remote.id, me.id]);
    assert_eq!(ids(reader.events_matching(&all(vec![Source::Mic], None))?), vec![me.id]);
    assert_eq!(ids(reader.events_matching(&all(vec![], Some(1_500)))?), vec![remote.id]);
    let oldest_first = EventFilter { newest_first: false, ..all(vec![], None) };
    assert_eq!(ids(reader.events_matching(&oldest_first)?), vec![me.id, remote.id]);
    assert_eq!(reader.event_summary(None, &[Source::Mic])?, (1, Some(1_000)));
    assert_eq!(reader.event_summary(Some(5_000), &[])?, (0, None));
    // Search works read-only (its tokenizer lives in TEMP tables) and honours the source filter.
    let hits = reader.search(&SearchQuery { text: "deck".into(), sources: vec![Source::Mic], from: None, to: None, limit: None })?;
    assert_eq!(hits.iter().map(|h| h.event_id).collect::<Vec<_>>(), vec![me.id]);
    // Newest-first search, and an id lookup that respects the filter.
    let by_time = reader.search_by_time(&SearchQuery { text: "deck".into(), sources: vec![], from: None, to: None, limit: None })?;
    assert_eq!(by_time.iter().map(|h| h.event_id).collect::<Vec<_>>(), vec![remote.id, me.id]);
    assert!(by_time.iter().all(|h| h.rank == 0.0));
    assert!(reader.get_event_matching(remote.id, None, &[Source::Mic])?.is_none());
    assert!(reader.get_event_matching(me.id, Some(1_001), &[])?.is_none());
    assert_eq!(reader.get_event_matching(me.id, Some(1_000), &[Source::Mic])?.map(|e| e.id), Some(me.id));
    // The app's later writes are visible.
    insert(&store, &session, 3_000, None)?;
    assert_eq!(reader.event_summary(None, &[])?.0, 3);
    // Nothing can be written through it.
    assert!(reader.save_settings(&Settings::default()).is_err());
    assert!(reader.start_session(4_000).is_err());
    Ok(())
}

#[test]
fn read_only_store_needs_an_existing_current_history() -> Result<()> {
    let empty = tempfile::tempdir()?;
    assert!(Store::open_read_only(empty.path()).is_err());
    assert!(!empty.path().join("tuldmi.db").exists(), "must not create a database");
    let (directory, store) = setup()?;
    store.connection.lock().pragma_update(None, "user_version", LATEST_SCHEMA + 1)?;
    assert!(Store::open_read_only(directory.path()).is_err());
    store.connection.lock().pragma_update(None, "user_version", LATEST_SCHEMA - 1)?;
    assert!(Store::open_read_only(directory.path()).is_err());
    Ok(())
}

#[test]
fn settings_without_mcp_load_with_access_off() -> Result<()> {
    let (_directory, store) = setup()?;
    store.connection.lock().execute(
        "INSERT INTO settings (key, value) VALUES ('settings', '{\"onboardingComplete\": true}')",
        [],
    )?;
    let settings = store.load_settings()?;
    assert!(settings.onboarding_complete);
    assert_eq!(settings.mcp, McpSettings::default());
    assert!(!settings.mcp.enabled && !settings.mcp.include_remote && !settings.mcp.allow_audio);
    Ok(())
}

#[test]
fn time_ordered_search_ignores_excluded_transcripts() -> Result<()> {
    // bm25 ranks with statistics from every row, so hidden rows can reorder visible ones
    // (MCP review). Ordered by time, the visible result is the same whatever is hidden.
    let visible = |hidden: &str| -> Result<Vec<i64>> {
        let (_directory, store) = setup()?;
        let session = store.start_session(0)?;
        let a = insert(&store, &session, 1_000, None)?;
        let b = insert(&store, &session, 2_000, None)?;
        store.save_transcript(a.id, &[NewSegment { start_ms: 0, end_ms: 1, text: "alpha alpha beta".into(), confidence: None , speaker_id: None}], None, "m")?;
        store.save_transcript(b.id, &[NewSegment { start_ms: 0, end_ms: 1, text: "alpha beta beta".into(), confidence: None , speaker_id: None}], None, "m")?;
        for at in [3_000, 4_000, 5_000] {
            let event = store.insert_event(&NewEvent { session_id: session.id, source: Source::System, started_at: at, ended_at: at + 1, audio: None })?;
            store.save_transcript(event.id, &[NewSegment { start_ms: 0, end_ms: 1, text: hidden.into(), confidence: None , speaker_id: None}], None, "m")?;
        }
        let query = SearchQuery { text: "alpha beta".into(), sources: vec![Source::Mic], from: None, to: None, limit: Some(1) };
        // Compare by time: ids differ between the two databases.
        Ok(store.search_by_time(&query)?.into_iter().map(|h| h.at).collect())
    };
    assert_eq!(visible("alpha alpha alpha alpha beta")?, vec![2_000], "the newest visible match");
    assert_eq!(visible("alpha alpha alpha alpha beta")?, visible("beta beta beta beta alpha")?);
    Ok(())
}

// ---- remote speakers (docs/design/speakers.md) ------------------------------------------------

fn noon() -> i64 {
    crate::speakers::tracker::local_day_start(chrono::Local::now().timestamp_millis()) + 12 * 3_600_000
}

fn event_at(store: &Store, session: &Session, source: Source, start: i64, end: i64) -> Result<SpeechEvent> {
    store.insert_event(&NewEvent { session_id: session.id, source, started_at: start, ended_at: end, audio: None })
}

fn segment(text: &str, speaker: Option<Uuid>) -> NewSegment {
    NewSegment { start_ms: 0, end_ms: 50, text: text.into(), confidence: None, speaker_id: speaker }
}

#[test]
fn conversation_bounds_follow_long_events_that_bridge_gaps() -> Result<()> {
    let (_dir, store) = setup()?;
    let session = store.start_session(0)?;
    let t0 = noon();
    // A spans five minutes; B sits inside it; C starts just after A ends. One conversation.
    event_at(&store, &session, Source::System, t0, t0 + 300_000)?;
    event_at(&store, &session, Source::Mic, t0 + 1_000, t0 + 2_000)?;
    event_at(&store, &session, Source::System, t0 + 300_001, t0 + 310_000)?;
    assert_eq!(store.conversation_bounds(t0 + 300_001)?, (t0, t0 + 300_001));
    // From inside, the whole conversation counts, later events included (a retry of B).
    assert_eq!(store.conversation_bounds(t0 + 1_000)?, (t0, t0 + 300_001));
    // Exactly the gap after the latest end starts a new conversation (the timeline's rule).
    let gap = crate::speakers::tracker::CONVERSATION_GAP_MS;
    event_at(&store, &session, Source::System, t0 + 310_000 + gap, t0 + 320_000 + gap)?;
    assert_eq!(store.conversation_bounds(t0 + 310_000 + gap)?, (t0 + 310_000 + gap, t0 + 310_000 + gap));
    assert_eq!(store.conversation_bounds(t0 + 300_001)?, (t0, t0 + 300_001), "the next conversation isn't included");
    Ok(())
}

#[test]
fn speaker_numbers_stay_within_their_conversation_and_are_never_reused() -> Result<()> {
    let (_dir, store) = setup()?;
    let session = store.start_session(0)?;
    let t0 = noon();
    let morning = event_at(&store, &session, Source::System, t0 - 3_600_000, t0 - 3_590_000)?;
    let afternoon = event_at(&store, &session, Source::System, t0, t0 + 10_000)?;
    let two = store.create_person_speaker("Speaker 2", 2, t0 - 3_600_000)?;
    let four = store.create_person_speaker("Speaker 4", 4, t0)?;
    store.save_transcript_with_speakers(afternoon.id, &[segment("a", Some(four.id))], None, "m", SpeakersState::Done)?;
    store.save_transcript_with_speakers(morning.id, &[segment("b", Some(two.id))], None, "m", SpeakersState::Done)?;
    // The morning conversation only counts its own speakers, not the afternoon's.
    assert_eq!(store.max_speaker_ordinal(t0 - 3_600_000, t0 - 3_590_000)?, 2);
    // A renamed speaker keeps its number; "Speaker 1" being gone doesn't make 2 reusable.
    store.rename_speaker(two.id, "Dana")?;
    assert_eq!(store.max_speaker_ordinal(t0 - 3_600_000, t0 - 3_590_000)?, 2);
    assert_eq!(store.max_speaker_ordinal(t0, t0 + 10_000)?, 4);
    // Deleting the event that showed Speaker 4 doesn't free the number.
    store.delete_event(afternoon.id)?;
    assert_eq!(store.max_speaker_ordinal(t0, t0 + 10_000)?, 4);
    Ok(())
}

#[test]
fn a_backfill_save_leaves_events_separated_meanwhile_alone() -> Result<()> {
    let (_dir, store) = setup()?;
    let session = store.start_session(0)?;
    let t0 = noon();
    let open = event_at(&store, &session, Source::System, t0, t0 + 1_000)?;
    let taken = event_at(&store, &session, Source::System, t0 + 2_000, t0 + 3_000)?;
    let (_, open_ids) = store.save_transcript_with_speakers(open.id, &[segment("x", None)], None, "m", SpeakersState::Done)
?;
    // `open` stays unseparated for the test: reset its state the way the backfill finds it.
    store.connection.lock().execute("UPDATE audio_events SET speakers_state = NULL WHERE id = ?1", [open.id.to_string()])?;
    let live = store.create_person_speaker("Speaker 1", 1, t0 + 2_000)?;
    let (_, taken_ids) = store.save_transcript_with_speakers(taken.id, &[segment("y", Some(live.id))], None, "m", SpeakersState::Done)?;

    let new = Uuid::now_v7();
    let unused = Uuid::now_v7();
    let touched = store.save_backfill(
        &[(new, "Speaker 1".into(), 1, t0), (unused, "Speaker 2".into(), 2, t0)],
        &[(open_ids[0], new), (taken_ids[0], new)],
        &[(open.id, SpeakersState::Done), (taken.id, SpeakersState::Done)],
    )?;
    assert_eq!(touched, vec![open.id]);
    let open = store.get_event(open.id)?.unwrap();
    assert_eq!(open.segments[0].speaker.as_ref().map(|s| s.id), Some(new));
    // The live-separated event keeps its own speaker.
    let taken = store.get_event(taken.id)?.unwrap();
    assert_eq!(taken.segments[0].speaker.as_ref().map(|s| s.id), Some(live.id));
    // A speaker nothing shows isn't kept.
    let kept: i64 = store.connection.lock().query_row("SELECT COUNT(*) FROM speakers WHERE id = ?1", [unused.to_string()], |r| r.get(0))?;
    assert_eq!(kept, 0);
    Ok(())
}

#[test]
fn orphaned_speakers_lose_their_name_but_keep_their_number() -> Result<()> {
    let (_dir, store) = setup()?;
    let session = store.start_session(0)?;
    let t0 = noon();
    let a = event_at(&store, &session, Source::System, t0, t0 + 1_000)?;
    let b = event_at(&store, &session, Source::System, t0 + 2_000, t0 + 3_000)?;
    // Backfilled in A+B: Speaker 2 first shown in A, still shown in B.
    let two = store.create_person_speaker("Speaker 2", 2, t0)?;
    store.rename_speaker(two.id, "Dana")?;
    store.save_transcript_with_speakers(a.id, &[segment("a", Some(two.id))], None, "m", SpeakersState::Done)?;
    store.save_transcript_with_speakers(b.id, &[segment("b", Some(two.id))], None, "m", SpeakersState::Done)?;
    store.delete_event(a.id)?;
    // A is gone, so the conversation now starts at B, after Speaker 2's first-shown time: it
    // still counts through B's segment.
    assert_eq!(store.conversation_bounds(t0 + 2_000)?, (t0 + 2_000, t0 + 2_000));
    assert_eq!(store.max_speaker_ordinal(t0 + 2_000, t0 + 2_000)?, 2);
    // Once nothing shows Dana, the typed name goes; the number stays reserved.
    store.delete_event(b.id)?;
    assert_eq!(store.forget_orphan_speaker_names(i64::MAX)?, 1);
    let label: String = store.connection.lock().query_row("SELECT label FROM speakers WHERE id = ?1", [two.id.to_string()], |r| r.get(0))?;
    assert_eq!(label, "Speaker 2");
    assert_eq!(store.max_speaker_ordinal(t0, t0 + 3_000)?, 2);
    Ok(())
}
