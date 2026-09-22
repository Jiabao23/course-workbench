mod common;
use common::{asset, segment};
use course_core::{quality::RecheckCandidate, Db, Transcript};
use serde_json::json;

fn setup() -> (tempfile::TempDir, Db, Transcript, RecheckCandidate) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("test.db")).unwrap();
    db.upsert_asset(&asset("a")).unwrap();
    let t = db
        .save_transcript(
            "a",
            "whisper",
            Some("small"),
            "zh",
            &[
                segment("before", 0, 1000, "beforeword"),
                segment("old", 1000, 2000, "oldword"),
                segment("after", 2000, 3000, "afterword"),
            ],
        )
        .unwrap();
    let c = RecheckCandidate {
        id: "candidate".into(),
        asset_id: "a".into(),
        transcript_id: t.id.clone(),
        start_ms: 1000,
        end_ms: 2000,
        model: "medium".into(),
        device: "cpu".into(),
        original_text: "oldword".into(),
        segments: vec![segment("replacement", 1000, 2000, "newword")],
        created_at: "2026-09-23T00:00:00Z".into(),
        audio_fingerprint: "a".repeat(64),
    };
    (dir, db, t, c)
}

#[test]
fn reviews_and_evidence_survive_reopen_without_legacy_review_conversion() {
    let (_dir, db, t, _) = setup();
    let fp = "a".repeat(64);
    db.save_integrity_review(&t.id, &fp, "legacy").unwrap();
    assert!(db.issue_reviews(&t.id, &fp).unwrap().is_empty());
    assert!(db
        .save_issue_review(&t.id, &fp, "issue", "confirmed", " ")
        .is_err());
    assert!(db
        .save_issue_review(&t.id, "", "issue", "pending", "")
        .is_err());
    assert!(db.save_issue_review(&t.id, &fp, "", "pending", "").is_err());
    assert!(db
        .save_issue_review(&t.id, &fp, "issue", "other", "note")
        .is_err());
    db.save_issue_review(&t.id, &fp, "issue", "confirmed", "checked")
        .unwrap();
    db.save_quality_evidence(&t.id, "diagnostics", &json!({"score":0.4}))
        .unwrap();
    db.save_quality_evidence(&t.id, "speech", &json!({"fingerprint":"first"}))
        .unwrap();
    db.save_quality_evidence(&t.id, "speech", &json!({"fingerprint":"second"}))
        .unwrap();
    assert_eq!(
        db.quality_evidence(&t.id, "speech").unwrap(),
        Some(json!({"fingerprint":"second"}))
    );
    assert!(db
        .save_quality_evidence(&t.id, "diagnostics", &json!({"score":0.7}))
        .is_err());
    let db = Db::open(db.path()).unwrap();
    assert_eq!(db.issue_reviews(&t.id, &fp).unwrap()[0].note, "checked");
    assert!(db.issue_reviews(&t.id, &"b".repeat(64)).unwrap().is_empty());
    assert_eq!(
        db.quality_evidence(&t.id, "diagnostics").unwrap(),
        Some(json!({"score":0.4}))
    );
}

#[test]
fn candidate_adoption_is_atomic_idempotent_and_preserves_history() {
    let (_dir, db, base, c) = setup();
    db.save_note(&course_core::Note {
        id: "note".into(),
        asset_id: "a".into(),
        transcript_id: base.id.clone(),
        kind: "manual".into(),
        title: "Historical reference".into(),
        content: "checked".into(),
        citations: vec![course_core::Citation {
            segment_id: base.segments[1].id.clone(),
            start_ms: 1000,
            end_ms: 2000,
            text: "oldword".into(),
        }],
        question: None,
        created_at: c.created_at.clone(),
        stale: false,
    })
    .unwrap();
    db.save_candidate(&c).unwrap();
    let db = Db::open(db.path()).unwrap();
    assert_eq!(db.list_candidates("a", &base.id).unwrap().len(), 1);
    let adopted = db.adopt_candidate(&c.id).unwrap();
    assert_eq!(adopted.version, 2);
    assert_eq!(adopted.model, None);
    assert_eq!(adopted.source_kind, "edited");
    assert_eq!(adopted.segments[0], base.segments[0]);
    assert_eq!(adopted.segments[2], base.segments[2]);
    assert_eq!(db.get_transcript(&base.id).unwrap().segments, base.segments);
    assert!(db.search("oldword", None).unwrap().is_empty());
    assert_eq!(db.search("newword", None).unwrap().len(), 1);
    let notes = db.list_notes("a").unwrap();
    assert!(notes[0].stale);
    assert_eq!(notes[0].citations[0].text, "oldword");
    assert!(db
        .quality_evidence(&adopted.id, "provenance")
        .unwrap()
        .is_some());
    assert_eq!(db.adopt_candidate(&c.id).unwrap().id, adopted.id);
    assert!(db.list_candidates("a", &base.id).unwrap().is_empty());
    assert!(db.discard_candidate(&c.id).is_err());
}

#[test]
fn candidate_validation_staleness_and_rollback() {
    let (_dir, db, base, c) = setup();
    db.upsert_asset(&asset("b")).unwrap();
    let mut bad = c.clone();
    bad.asset_id = "b".into();
    assert!(db.save_candidate(&bad).is_err());
    bad = c.clone();
    bad.start_ms = 1100;
    assert!(db.save_candidate(&bad).is_err());
    bad = c.clone();
    bad.end_ms = 121001;
    assert!(db.save_candidate(&bad).is_err());
    bad = c.clone();
    bad.segments[0].id = "before".into();
    assert!(db.save_candidate(&bad).is_err());
    db.save_candidate(&c).unwrap();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_provenance BEFORE INSERT ON quality_evidence BEGIN SELECT RAISE(ABORT,'simulated failure'); END;").unwrap();
    assert!(db.adopt_candidate(&c.id).is_err());
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, base.id);
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(db.search("oldword", None).unwrap().len(), 1);
    conn.execute_batch("DROP TRIGGER reject_provenance;")
        .unwrap();
    db.save_transcript("a", "edited", None, "zh", &base.segments)
        .unwrap();
    assert!(db.adopt_candidate(&c.id).is_err());
    db.discard_candidate(&c.id).unwrap();
    assert!(db.get_candidate(&c.id).is_err());
}

#[test]
fn migration_from_four_preserves_references() {
    let (_dir, db, t, _) = setup();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("DROP TABLE recheck_candidates; DROP TABLE issue_reviews; DROP TABLE quality_evidence; DROP TABLE quality_evidence_history; PRAGMA user_version=4;").unwrap();
    let db = Db::open(db.path()).unwrap();
    assert_eq!(
        conn.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        5
    );
    assert_eq!(db.get_transcript(&t.id).unwrap().segments, t.segments);
    assert!(db
        .save_quality_evidence("missing", "speech", &json!({}))
        .is_err());
    assert!(db
        .save_quality_evidence(&t.id, "unknown", &json!({}))
        .is_err());
}

#[test]
fn speech_history_is_archived_once_and_retains_review_evidence() {
    let (_dir, db, t, _) = setup();
    let first = json!({"fingerprint":"first","intervals":[[1000,2000]]});
    let second = json!({"fingerprint":"second"});
    db.save_quality_evidence(&t.id, "speech", &first).unwrap();
    db.save_issue_review(&t.id, "first", "issue", "confirmed", "heard it")
        .unwrap();
    db.save_quality_evidence(&t.id, "speech", &second).unwrap();
    db.save_quality_evidence(&t.id, "speech", &second).unwrap();
    assert_eq!(
        db.quality_evidence_history(&t.id, "speech").unwrap(),
        vec![first.clone()]
    );
    db.save_quality_evidence(&t.id, "speech", &first).unwrap();
    db.save_quality_evidence(&t.id, "speech", &second).unwrap();
    let db = Db::open(db.path()).unwrap();
    assert_eq!(
        db.quality_evidence_history(&t.id, "speech").unwrap().len(),
        2
    );
    assert_eq!(
        db.issue_reviews(&t.id, "first").unwrap()[0].note,
        "heard it"
    );
    assert!(db.latest_integrity_review(&t.id).unwrap().is_none());
    db.save_integrity_review(&t.id, &"a".repeat(64), "historical summary")
        .unwrap();
    assert_eq!(
        db.latest_integrity_review(&t.id).unwrap().unwrap().0,
        "historical summary"
    );
}

#[test]
fn revised_issue_records_commit_with_adoption_and_rollback_on_failure() {
    let (_dir, db, base, c) = setup();
    db.save_candidate(&c).unwrap();
    assert!(db
        .save_issue_review(&base.id, "fp", "issue", "revised", "")
        .is_err());
    db.save_issue_review(&base.id, "fp", "issue", "pending", "")
        .unwrap();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_revised BEFORE UPDATE ON issue_reviews WHEN NEW.status='revised' BEGIN SELECT RAISE(ABORT,'simulated review failure'); END;").unwrap();
    assert!(db
        .adopt_candidate_with_reviews(&c.id, "fp", &["issue".into()])
        .is_err());
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, base.id);
    assert_eq!(db.list_candidates("a", &base.id).unwrap().len(), 1);
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(
        db.issue_reviews(&base.id, "fp").unwrap()[0].status,
        "pending"
    );
    conn.execute_batch("DROP TRIGGER reject_revised;").unwrap();
    let adopted = db
        .adopt_candidate_with_reviews(&c.id, "fp", &["issue".into()])
        .unwrap();
    let reviews = db.issue_reviews(&base.id, "fp").unwrap();
    assert_eq!(reviews[0].status, "revised");
    assert!(reviews[0].note.contains(&adopted.id));
    assert_eq!(
        db.adopt_candidate_with_reviews(&c.id, "fp", &["other".into()])
            .unwrap()
            .id,
        adopted.id
    );
    assert_eq!(db.issue_reviews(&base.id, "fp").unwrap(), reviews);
}

#[test]
fn speech_replacement_and_archiving_roll_back_together() {
    let (_dir, db, t, _) = setup();
    let first = json!({"fingerprint":"first"});
    db.save_quality_evidence(&t.id, "speech", &first).unwrap();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("CREATE TRIGGER reject_speech BEFORE UPDATE ON quality_evidence WHEN NEW.kind='speech' BEGIN SELECT RAISE(ABORT,'simulated speech failure'); END;").unwrap();
    assert!(db
        .save_quality_evidence(&t.id, "speech", &json!({"fingerprint":"second"}))
        .is_err());
    assert_eq!(db.quality_evidence(&t.id, "speech").unwrap(), Some(first));
    assert!(db
        .quality_evidence_history(&t.id, "speech")
        .unwrap()
        .is_empty());
}

#[test]
fn manual_edits_only_revise_fully_covered_text_issues() {
    use course_core::quality::EditIssue;
    let (_dir, db, base, _) = setup();
    let issues = vec![
        EditIssue {
            id: "changed".into(),
            code: "recognitionDoubt".into(),
            start_ms: 1000,
            end_ms: 2000,
        },
        EditIssue {
            id: "unchanged".into(),
            code: "repetition".into(),
            start_ms: 0,
            end_ms: 1000,
        },
        EditIssue {
            id: "partial".into(),
            code: "repetition".into(),
            start_ms: 1000,
            end_ms: 3000,
        },
        EditIssue {
            id: "gap".into(),
            code: "gap".into(),
            start_ms: 1000,
            end_ms: 2000,
        },
        EditIssue {
            id: "unknown".into(),
            code: "unknownChunks".into(),
            start_ms: 1000,
            end_ms: 2000,
        },
    ];
    let mut segments = base.segments.clone();
    segments[1].text = "correctedword".into();
    let conn = rusqlite::Connection::open(db.path()).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_edit BEFORE INSERT ON issue_reviews BEGIN SELECT RAISE(ABORT,'simulated edit review failure'); END;").unwrap();
    assert!(db
        .save_edit_with_reviews("a", &base.id, &segments, "fp", &issues)
        .is_err());
    assert_eq!(db.active_transcript("a").unwrap().unwrap().id, base.id);
    assert_eq!(db.list_transcripts("a").unwrap().len(), 1);
    assert_eq!(db.search("oldword", None).unwrap().len(), 1);
    conn.execute_batch("DROP TRIGGER fail_edit;").unwrap();
    let edited = db
        .save_edit_with_reviews("a", &base.id, &segments, "fp", &issues)
        .unwrap();
    let reviews = db.issue_reviews(&base.id, "fp").unwrap();
    assert_eq!(reviews.len(), 1);
    assert_eq!(reviews[0].issue_id, "changed");
    assert_eq!(reviews[0].status, "revised");
    assert!(reviews[0].note.contains(&edited.id));
    assert_eq!(edited.segments[1].id, base.segments[1].id);
    assert_eq!(db.get_transcript(&base.id).unwrap().segments, base.segments);
    assert!(db
        .quality_evidence(&edited.id, "provenance")
        .unwrap()
        .is_some());
    assert!(db
        .save_edit_with_reviews("a", &base.id, &segments, "fp", &issues)
        .is_err());
}

#[test]
fn unchanged_text_and_timing_only_edits_never_resolve_issues() {
    use course_core::quality::EditIssue;
    let (_dir, db, base, _) = setup();
    let issues = vec![EditIssue {
        id: "issue".into(),
        code: "recognitionDoubt".into(),
        start_ms: 1000,
        end_ms: 2000,
    }];
    let mut altered = base.segments.clone();
    altered[1].start_ms = 1100;
    assert!(db
        .save_edit_with_reviews("a", &base.id, &altered, "fp", &issues)
        .is_err());
    let mut whitespace = base.segments.clone();
    whitespace[1].text = format!(" {} ", whitespace[1].text);
    let unchanged = db
        .save_edit_with_reviews("a", &base.id, &whitespace, "fp", &issues)
        .unwrap();
    assert!(db.issue_reviews(&base.id, "fp").unwrap().is_empty());
    assert_eq!(unchanged.segments, whitespace);
}
