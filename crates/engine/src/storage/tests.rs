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
        },
        NewSegment {
            start_ms: 10,
            end_ms: 20,
            text: "reject".into(),
            confidence: None,
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
                },
                NewSegment {
                    start_ms: 20,
                    end_ms: 30,
                    text: "other".into(),
                    confidence: None,
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
