use course_core::{Asset, Segment, Transcript};
use course_workbench_lib::integrity::{check, require_complete_chunks};

fn sample() -> (Asset, Transcript) {
    let asset: Asset = serde_json::from_value(serde_json::json!({"id":"asset-1","title":"课程","sourceKind":"localMedia","source":"D:/lesson.wav","bvid":null,"page":null,"durationMs":60000,"audioPath":null,"activeVersionId":"version-1","createdAt":"now","updatedAt":"now"})).unwrap();
    let transcript = Transcript {
        id: "version-1".into(),
        asset_id: asset.id.clone(),
        version: 1,
        source_kind: "whisper".into(),
        model: Some("small".into()),
        language: "zh".into(),
        segments: vec![
            seg("a", 0, 10000, "第一节"),
            seg("b", 30000, 35000, "第二节"),
        ],
        created_at: "now".into(),
        is_active: true,
    };
    (asset, transcript)
}
fn seg(id: &str, start_ms: u64, end_ms: u64, text: &str) -> Segment {
    Segment {
        id: id.into(),
        start_ms,
        end_ms,
        text: text.into(),
    }
}

#[test]
fn reports_missing_middle_and_tail_without_claiming_word_accuracy() {
    let (a, t) = sample();
    let r = check(&a, &t, None, None);
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "gap" && i.start_ms == 10000 && i.end_ms == 30000));
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "tail" && i.start_ms == 35000));
    assert_eq!(r.covered_ms, 15000);
    assert_eq!(r.status, "needsReview");
    assert!(r.limitations.contains("逐字"));
}
#[test]
fn subtitles_do_not_invent_an_independent_source_duration() {
    let (mut a, mut t) = sample();
    a.source_kind = "subtitle".into();
    a.duration_ms = 35000;
    t.source_kind = "subtitle".into();
    let r = check(&a, &t, None, None);
    assert_eq!(r.duration_ms, None);
    assert!(r.issues.iter().any(|i| i.code == "unknownDuration"));
    assert!(!r.issues.iter().any(|i| i.code == "tail"));
}
#[test]
fn overlapping_segments_use_union_and_repeated_text_is_flagged() {
    let (mut a, mut t) = sample();
    a.duration_ms = 30000;
    t.segments = vec![
        seg("a", 0, 12000, "重复"),
        seg("b", 9000, 20000, "重复"),
        seg("c", 20000, 30000, "重复"),
    ];
    let r = check(&a, &t, None, None);
    assert_eq!(r.covered_ms, 30000);
    assert!(r.issues.iter().any(|i| i.code == "overlap"));
    assert!(r.issues.iter().any(|i| i.code == "repetition"));
}
#[test]
fn unknown_or_incomplete_chunks_cannot_be_committed_as_complete() {
    assert!(require_complete_chunks(0, 0, 60000).is_err());
    assert!(require_complete_chunks(1, 2, 356608).is_err());
    assert!(require_complete_chunks(1, 1, 356608).is_err());
    assert!(require_complete_chunks(2, 2, 356608).is_ok());
}
#[test]
fn duration_change_invalidates_review_and_truncated_download_is_visible() {
    let (mut a, t) = sample();
    let first = check(&a, &t, None, Some(120000));
    assert!(first
        .issues
        .iter()
        .any(|i| i.code == "mediaDurationMismatch"));
    a.duration_ms = 120000;
    let second = check(&a, &t, None, Some(120000));
    assert_ne!(first.fingerprint, second.fingerprint);
}
#[test]
fn clean_timeline_still_does_not_claim_content_is_complete() {
    let (a, mut t) = sample();
    t.source_kind = "webSubtitle".into();
    t.segments = vec![seg("a", 0, 60000, "课程正文")];
    let r = check(&a, &t, None, None);
    assert_eq!(r.status, "noObviousIssues");
    assert!(r.review.is_none());
}

#[test]
fn acquiring_same_duration_audio_invalidates_previous_report() {
    let (mut a, t) = sample();
    let before = check(&a, &t, None, None);
    a.audio_path = Some("D:/new-audio.wav".into());
    let after = check(&a, &t, None, None);
    assert_ne!(before.fingerprint, after.fingerprint);
}
#[test]
fn edited_version_does_not_erase_missing_processing_evidence() {
    let (a, mut t) = sample();
    t.source_kind = "edited".into();
    t.segments = vec![seg("a", 0, 60000, "校对文本")];
    let r = check(&a, &t, None, None);
    assert!(r.issues.iter().any(|i| i.code == "unknownChunks"));
}

#[test]
fn submillisecond_remainder_after_chunk_boundary_is_not_rejected() {
    // 4,800,001 PCM samples at 16 kHz rounds to 300,000 ms but has 2 chunks.
    assert!(require_complete_chunks(2, 2, 300000).is_ok());
    assert!(require_complete_chunks(3, 3, 300000).is_err());
    assert!(require_complete_chunks(2, 2, 299999).is_err());
}
