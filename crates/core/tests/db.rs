mod common;

use common::{asset, job, segment};
use course_core::{
    db::Db,
    types::{Citation, Note},
};
use std::sync::Arc;

fn database() -> (tempfile::TempDir, Db) {
    let directory = tempfile::tempdir().unwrap();
    let db = Db::open(&directory.path().join("workbench.sqlite3")).unwrap();
    (directory, db)
}

fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn job_result_and_completion_are_committed_once_across_reopen() {
    let (directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    db.upsert_job(&job("j", "a", "running")).unwrap();
    let segments = vec![segment("s", 0, 1000, "已完成的原文")];
    let first = db
        .save_job_transcript("j", "subtitle", None, "zh", &segments)
        .unwrap();
    let reopened = Db::open(&directory.path().join("workbench.sqlite3")).unwrap();
    assert_eq!(reopened.recover_jobs().unwrap(), 0);
    assert_eq!(reopened.get_job("j").unwrap().status, "completed");
    let edited = reopened
        .save_transcript(
            "a",
            "edited",
            None,
            "zh",
            &[segment("s", 0, 1000, "修订原文")],
        )
        .unwrap();
    let repeated = reopened
        .save_job_transcript("j", "subtitle", None, "zh", &segments)
        .unwrap();
    assert_eq!(first.id, repeated.id);
    assert_eq!(reopened.list_transcripts("a").unwrap().len(), 2);
    assert_eq!(
        reopened.active_transcript("a").unwrap().unwrap().id,
        edited.id
    );
}

#[test]
fn failing_terminal_job_write_rolls_back_the_new_transcript_and_index() {
    let (_temp, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let original = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "原来文字")],
        )
        .unwrap();
    db.upsert_job(&job("j", "a", "running")).unwrap();
    let connection = rusqlite::Connection::open(db.path()).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_completion BEFORE UPDATE ON jobs WHEN NEW.status='completed' BEGIN SELECT RAISE(ABORT,'simulated disk failure'); END;").unwrap();
    assert!(db
        .save_job_transcript(
            "j",
            "whisper",
            Some("small"),
            "zh",
            &[segment("s", 0, 1000, "新的文字")]
        )
        .is_err());
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, original.id);
    assert_eq!(db.get_job("j").unwrap().status, "running");
    assert!(db.search("新的", None).unwrap().is_empty());
}

#[test]
fn batch_enqueue_is_atomic_when_a_later_asset_is_already_running() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("busy")).unwrap();
    db.upsert_job(&job("running", "busy", "running")).unwrap();
    let batch = vec![
        (asset("new"), job("new-job", "new", "queued")),
        (asset("busy"), job("conflict", "busy", "queued")),
    ];
    assert!(db.enqueue_jobs(&batch).is_err());
    assert!(db.get_asset("new").is_err());
    assert_eq!(db.list_jobs().unwrap().len(), 1);
    db.enqueue_jobs(&batch[..1]).unwrap();
    assert_eq!(db.list_jobs().unwrap().len(), 2);
}

#[test]
fn db_is_send_sync_and_reopens_with_assets_intact() {
    assert_send_sync::<Db>();
    let (directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let reopened = Db::open(&directory.path().join("workbench.sqlite3")).unwrap();
    assert_eq!(reopened.get_asset("a").unwrap(), asset("a"));
    assert_eq!(reopened.list_assets().unwrap().len(), 1);
    assert!(reopened.get_asset("missing").is_err());
    let connection = rusqlite::Connection::open(db.path()).unwrap();
    let mode: String = connection
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
}

#[test]
fn saves_immutable_versions_and_searches_only_the_active_version() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let old = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[
                segment("s1", 1234, 4000, "量子力学讨论电子与光子。"),
                segment("s2", 5000, 9000, "实验记录与测量。"),
            ],
        )
        .unwrap();
    assert_eq!(old.version, 1);
    assert!(old.is_active);
    let hit = db.search("量子力学", None).unwrap();
    assert_eq!(hit.len(), 1);
    assert_eq!(hit[0].segment_id, "s1");
    assert_eq!(hit[0].transcript_id, old.id);
    assert_eq!((hit[0].start_ms, hit[0].end_ms), (1234, 4000));

    let current = db
        .save_transcript(
            "a",
            "edit",
            None,
            "zh",
            &[segment("s1", 1234, 4000, "热力学研究能量守恒。")],
        )
        .unwrap();
    assert_eq!(current.version, 2);
    assert_ne!(current.id, old.id);
    assert!(!db.get_transcript(&old.id).unwrap().is_active);
    assert_eq!(db.get_transcript(&old.id).unwrap().segments, old.segments);
    assert_eq!(current.segments[0].id, "s1", "editing retains supplied IDs");
    assert!(db.search("量子力学", None).unwrap().is_empty());
    assert_eq!(db.search("热力学", None).unwrap().len(), 1);

    db.activate_transcript("a", &old.id).unwrap();
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, old.id);
    assert_eq!(
        db.list_transcripts("a")
            .unwrap()
            .iter()
            .filter(|version| version.is_active)
            .count(),
        1
    );
    assert_eq!(db.search("量子力学", None).unwrap().len(), 1);
    assert!(db.search("热力学", None).unwrap().is_empty());
}

#[test]
fn chinese_search_is_scoped_and_fts_operators_are_literal_input() {
    let (_directory, db) = database();
    for id in ["a", "b"] {
        db.upsert_asset(&asset(id)).unwrap();
        db.save_transcript(
            id,
            "asr",
            Some("base"),
            "zh",
            &[segment(
                "first",
                0,
                1000,
                "机器学习包含监督学习方法。 rust compiler",
            )],
        )
        .unwrap();
    }
    assert_eq!(db.search("机器学习", None).unwrap().len(), 2);
    let scoped = db.search("机器学习", Some("a")).unwrap();
    assert_eq!(scoped.len(), 1);
    assert_eq!(scoped[0].asset_id, "a");
    assert_eq!(db.search("监督", Some("b")).unwrap().len(), 1);
    assert!(db.search("", None).unwrap().is_empty());
    assert!(db.search("  \" * : ( )  ", None).unwrap().is_empty());
    assert!(db.search("\" OR nonexistent : *", None).is_ok());
    assert_eq!(db.search("RUST", None).unwrap().len(), 2);
}

#[test]
fn invalid_version_and_cross_asset_activation_leave_active_search_unchanged() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    db.upsert_asset(&asset("b")).unwrap();
    let first = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "量子力学")],
        )
        .unwrap();
    let other = db
        .save_transcript(
            "b",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "热力学")],
        )
        .unwrap();
    assert!(db
        .save_transcript("a", "edit", None, "zh", &[segment("s", 1000, 100, "错误")])
        .is_err());
    assert!(db
        .save_transcript(
            "a",
            "edit",
            None,
            "zh",
            &[
                segment("same", 0, 100, "一"),
                segment("same", 100, 200, "二")
            ]
        )
        .is_err());
    assert!(db.activate_transcript("a", &other.id).is_err());
    assert!(db.activate_transcript("a", "missing").is_err());
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, first.id);
    assert_eq!(db.search("量子力学", Some("a")).unwrap().len(), 1);
    assert!(db.search("热力学", Some("a")).unwrap().is_empty());
}

#[test]
fn note_staleness_is_computed_from_current_active_version() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let first = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 123, 1000, "证据文本")],
        )
        .unwrap();
    let note = Note {
        id: "n1".into(),
        asset_id: "a".into(),
        transcript_id: first.id.clone(),
        kind: "summary".into(),
        title: "摘要".into(),
        content: "摘要内容".into(),
        citations: vec![Citation {
            segment_id: "s".into(),
            start_ms: 123,
            end_ms: 1000,
            text: "证据文本".into(),
        }],
        question: None,
        created_at: "2026-09-09T00:00:00Z".into(),
        stale: true,
    };
    db.save_note(&note).unwrap();
    assert!(
        !db.list_notes("a").unwrap()[0].stale,
        "input stale flag must not be authoritative"
    );
    db.save_transcript(
        "a",
        "edit",
        None,
        "zh",
        &[segment("s", 123, 1000, "修改文本")],
    )
    .unwrap();
    let historical = db.list_notes("a").unwrap();
    assert!(historical[0].stale);
    assert_eq!(historical[0].citations[0].text, "证据文本");
    db.activate_transcript("a", &first.id).unwrap();
    assert!(!db.list_notes("a").unwrap()[0].stale);
}

#[test]
fn notes_cannot_attach_another_assets_version_or_fabricated_citations() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    db.upsert_asset(&asset("b")).unwrap();
    let version = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "证据")],
        )
        .unwrap();
    let mut note = Note {
        id: "n".into(),
        asset_id: "b".into(),
        transcript_id: version.id,
        kind: "manual".into(),
        title: "笔记".into(),
        content: "正文".into(),
        citations: vec![],
        question: None,
        created_at: "2026-09-09T00:00:00Z".into(),
        stale: false,
    };
    assert!(db.save_note(&note).is_err());
    note.asset_id = "a".into();
    note.citations.push(Citation {
        segment_id: "invented".into(),
        start_ms: 0,
        end_ms: 1000,
        text: "假证据".into(),
    });
    assert!(db.save_note(&note).is_err());
    note.citations = vec![];
    db.save_note(&note).unwrap();
}

#[test]
fn startup_recovery_pauses_interrupted_jobs_and_keeps_checkpoints() {
    let (directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    for status in [
        "running",
        "queued",
        "completed",
        "failed",
        "cancelled",
        "paused",
    ] {
        db.upsert_job(&job(status, "a", status)).unwrap();
    }
    let reopened = Db::open(&directory.path().join("workbench.sqlite3")).unwrap();
    assert_eq!(reopened.recover_jobs().unwrap(), 2);
    assert_eq!(reopened.recover_jobs().unwrap(), 0);
    for previous in ["running", "queued"] {
        let recovered = reopened.get_job(previous).unwrap();
        assert_eq!(recovered.status, "paused");
        assert_eq!((recovered.chunk_done, recovered.chunk_total), (2, 5));
        assert_eq!(recovered.progress, 40.0);
        assert_eq!(recovered.model, "small");
        assert_eq!(recovered.device, "cuda");
        assert_eq!(recovered.stage, "transcribing");
    }
    for unchanged in ["completed", "failed", "cancelled", "paused"] {
        assert_eq!(reopened.get_job(unchanged).unwrap().status, unchanged);
    }
    assert_eq!(reopened.list_jobs().unwrap().len(), 6);
}

#[test]
fn invalid_job_progress_never_corrupts_persisted_checkpoint() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let valid = job("j", "a", "running");
    db.upsert_job(&valid).unwrap();
    for progress in [-0.1, 100.1, f64::NAN, f64::INFINITY] {
        let mut invalid = valid.clone();
        invalid.progress = progress;
        assert!(db.upsert_job(&invalid).is_err());
    }
    let mut invalid = valid.clone();
    invalid.chunk_done = 6;
    assert!(db.upsert_job(&invalid).is_err());
    assert_eq!(db.get_job("j").unwrap(), valid);
}

#[test]
fn concurrent_transcript_saves_get_unique_sequential_versions() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let db = Arc::new(db);
    let handles: Vec<_> = (0..4)
        .map(|number| {
            let db = Arc::clone(&db);
            std::thread::spawn(move || {
                db.save_transcript(
                    "a",
                    "edit",
                    None,
                    "zh",
                    &[segment("s", 0, 1000, &format!("并发保存 {number}"))],
                )
                .unwrap()
            })
        })
        .collect();
    let mut numbers: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().version)
        .collect();
    numbers.sort_unstable();
    assert_eq!(numbers, vec![1, 2, 3, 4]);
    let versions = db.list_transcripts("a").unwrap();
    assert_eq!(
        versions.iter().filter(|version| version.is_active).count(),
        1
    );
    assert_eq!(db.active_transcript("a").unwrap().unwrap().version, 4);
    assert_eq!(db.search("并发", Some("a")).unwrap().len(), 1);
}

#[test]
fn metadata_upsert_does_not_reset_the_transcript_pointer() {
    let (_directory, db) = database();
    let mut metadata = asset("a");
    db.upsert_asset(&metadata).unwrap();
    let version = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "量子力学")],
        )
        .unwrap();
    metadata.title = "修正标题".into();
    db.upsert_asset(&metadata).unwrap();
    assert_eq!(
        db.get_asset("a").unwrap().active_version_id,
        Some(version.id)
    );
    assert_eq!(
        db.search("量子力学", None).unwrap()[0].asset_title,
        "修正标题"
    );
}

#[test]
fn stale_metadata_from_a_background_job_cannot_replace_a_new_active_version() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    db.save_transcript(
        "a",
        "subtitle",
        None,
        "zh",
        &[segment("s", 0, 1000, "量子力学")],
    )
    .unwrap();
    let mut job_snapshot = db.get_asset("a").unwrap();
    let newer = db
        .save_transcript("a", "edit", None, "zh", &[segment("s", 0, 1000, "热力学")])
        .unwrap();
    job_snapshot.audio_path = Some("audio.wav".into());
    db.upsert_asset(&job_snapshot).unwrap();
    let updated = db.get_asset("a").unwrap();
    assert_eq!(updated.active_version_id, Some(newer.id));
    assert_eq!(updated.audio_path.as_deref(), Some("audio.wav"));
    assert_eq!(db.search("热力学", Some("a")).unwrap().len(), 1);
    assert!(db.search("量子力学", Some("a")).unwrap().is_empty());
}

#[test]
fn replacing_one_assets_index_does_not_remove_another_assets_hits() {
    let (_directory, db) = database();
    for id in ["a", "b"] {
        db.upsert_asset(&asset(id)).unwrap();
        db.save_transcript(
            id,
            "subtitle",
            None,
            "zh",
            &[segment("same", 0, 1000, "量子力学")],
        )
        .unwrap();
    }
    db.save_transcript(
        "a",
        "edit",
        None,
        "zh",
        &[segment("same", 0, 1000, "热力学")],
    )
    .unwrap();
    let remaining = db.search("量子力学", None).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].asset_id, "b");
}

#[test]
fn sqlite_failure_rolls_back_new_version_pointer_and_fts_together() {
    let (_directory, db) = database();
    db.upsert_asset(&asset("a")).unwrap();
    let old = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "量子力学")],
        )
        .unwrap();
    let connection = rusqlite::Connection::open(db.path()).unwrap();
    connection.execute_batch("CREATE TRIGGER reject_version_change BEFORE UPDATE OF active_version_id ON assets BEGIN SELECT RAISE(ABORT, 'simulated disk write failure'); END;").unwrap();
    assert!(db
        .save_transcript("a", "edit", None, "zh", &[segment("s", 0, 1000, "热力学")])
        .is_err());
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, old.id);
    assert_eq!(db.search("量子力学", None).unwrap().len(), 1);
    assert!(db.search("热力学", None).unwrap().is_empty());
}
