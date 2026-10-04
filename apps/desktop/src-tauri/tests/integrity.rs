use course_core::{Asset, Segment, Transcript};
use course_workbench_lib::integrity::{check, require_complete_chunks};

#[test]
fn supplied_invalid_manifest_cannot_fall_back_to_legacy_counts() {
    use course_workbench_lib::integrity::require_asr_completion;
    assert!(require_asr_completion(&serde_json::json!({}), 1, 1, 60_000).is_ok());
    for malformed in [
        serde_json::json!(null),
        serde_json::json!({}),
        serde_json::json!("bad"),
    ] {
        assert!(
            require_asr_completion(&serde_json::json!({"manifest":malformed}), 1, 1, 60_000)
                .is_err()
        );
    }
}

#[test]
fn manifest_completion_requires_full_ownership_and_each_completed_index() {
    use course_workbench_lib::integrity::require_complete_manifest;
    let manifest = serde_json::json!([
        {"index":0,"start_ms":0,"end_ms":290000,"decode_start_ms":0,"decode_end_ms":292000},
        {"index":1,"start_ms":290000,"end_ms":356608,"decode_start_ms":288000,"decode_end_ms":356608}
    ]);
    assert!(require_complete_manifest(&manifest, &serde_json::json!([0, 1]), 2, 2, 356608).is_ok());
    assert!(
        require_complete_manifest(&manifest, &serde_json::json!([0, 0]), 2, 2, 356608).is_err()
    );
    assert!(require_complete_manifest(&manifest, &serde_json::json!([0]), 1, 2, 356608).is_err());
    let mut gap = manifest.clone();
    gap[1]["start_ms"] = serde_json::json!(290001);
    assert!(require_complete_manifest(&gap, &serde_json::json!([0, 1]), 2, 2, 356608).is_err());
    let mut truncated = manifest.clone();
    truncated[1]["end_ms"] = serde_json::json!(356000);
    assert!(
        require_complete_manifest(&truncated, &serde_json::json!([0, 1]), 2, 2, 356608).is_err()
    );
    let mut invalid = manifest;
    invalid[0]["decode_start_ms"] = serde_json::json!(1);
    assert!(require_complete_manifest(&invalid, &serde_json::json!([0, 1]), 2, 2, 356608).is_err());
}

#[test]
fn duplicate_diagnostic_ids_are_rejected_before_enriching_the_report() {
    let (a, t) = sample();
    let mut report = check(&a, &t, None, None);
    let before = serde_json::to_value(&report).unwrap();
    let diagnostics = serde_json::json!([
        {"id":"a","diagnostics":{"avg_logprob":-0.2,"compression_ratio":1.0,"no_speech_prob":0.1}},
        {"id":"a","diagnostics":{"avg_logprob":-1.4,"compression_ratio":3.1,"no_speech_prob":0.9}}
    ]);
    let speech = serde_json::json!({"speech":[{"start_ms":18000,"end_ms":25000}]});
    let error = course_workbench_lib::integrity::add_quality_evidence(
        &mut report,
        &t,
        Some(&speech),
        Some(&diagnostics),
    )
    .expect_err("duplicate diagnostic IDs must not choose an arbitrary observation");
    assert!(error.to_string().contains("重复"));
    assert_eq!(serde_json::to_value(&report).unwrap(), before);
}

#[test]
fn enrichment_preserves_issue_order_and_quality_fingerprint() {
    use sha2::{Digest, Sha256};

    let (a, t) = sample();
    let mut report = check(&a, &t, None, None);
    let original_fingerprint = report.fingerprint.clone();
    let speech = serde_json::json!({"speech":[
        {"start_ms":10000,"end_ms":13000},
        {"start_ms":14500,"end_ms":30000},
        {"start_ms":35000,"end_ms":39000}
    ]});
    // Keep diagnostics in their evidence order, even when transcript order differs.
    let diagnostics = serde_json::json!([
        {"id":"b","diagnostics":{"avg_logprob":-1.5,"compression_ratio":1.0,"no_speech_prob":0.1}},
        {"id":"unrelated","diagnostics":{"avg_logprob":-9.0}},
        {"id":"a","diagnostics":{"avg_logprob":-0.2,"compression_ratio":3.0,"no_speech_prob":0.1}}
    ]);
    course_workbench_lib::integrity::add_quality_evidence(
        &mut report,
        &t,
        Some(&speech),
        Some(&diagnostics),
    )
    .unwrap();
    assert_eq!(
        report
            .issues
            .iter()
            .map(|i| (i.id.as_str(), i.severity.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("unknownChunks:0:0", "warning"),
            ("gap:10000:30000", "warning"),
            ("tail:35000:60000", "warning"),
            ("uncoveredSpeech:10000:13000", "warning"),
            ("uncoveredSpeech:14500:30000", "warning"),
            ("uncoveredSpeech:35000:39000", "warning"),
            ("recognitionDoubt:30000:35000", "warning"),
            ("recognitionDoubt:0:10000", "warning"),
        ]
    );
    assert_eq!(report.pending_count, 8);
    assert!(report.diagnostics_available);
    let expected_fingerprint = hex::encode(Sha256::digest(
        serde_json::to_vec(&(
            original_fingerprint,
            "quality-v1",
            Some(&speech),
            Some(&diagnostics),
        ))
        .unwrap(),
    ));
    assert_eq!(report.fingerprint, expected_fingerprint);

    course_workbench_lib::integrity::add_quality_evidence(
        &mut report,
        &t,
        None,
        Some(&diagnostics),
    )
    .unwrap();
    assert_eq!(
        report.issues.len(),
        8,
        "existing issue IDs remain deduplicated"
    );
}

#[test]
fn touching_speech_does_not_raise_silent_gap_priority() {
    let (a, t) = sample();
    let mut report = check(&a, &t, None, None);
    let speech = serde_json::json!({"speech":[
        {"start_ms":0,"end_ms":10000},
        {"start_ms":30000,"end_ms":35000}
    ]});
    course_workbench_lib::integrity::add_quality_evidence(&mut report, &t, Some(&speech), None)
        .unwrap();
    assert_eq!(
        report
            .issues
            .iter()
            .map(|i| (i.code.as_str(), i.severity.as_str()))
            .collect::<Vec<_>>(),
        vec![
            ("unknownChunks", "warning"),
            ("gap", "info"),
            ("tail", "info")
        ]
    );
    assert_eq!(report.pending_count, 1);
}

#[test]
fn incomplete_diagnostics_stay_unavailable_and_thresholds_remain_strict() {
    let (a, t) = sample();
    let mut report = check(&a, &t, None, None);
    let diagnostics = serde_json::json!([
        {"id":"a","diagnostics":{"avg_logprob":-1.0,"compression_ratio":2.4,"no_speech_prob":0.6}},
        {"id":"b","diagnostics":{"avg_logprob":-0.1,"compression_ratio":1.0}}
    ]);
    course_workbench_lib::integrity::add_quality_evidence(
        &mut report,
        &t,
        None,
        Some(&diagnostics),
    )
    .unwrap();
    assert!(!report.diagnostics_available);
    assert!(report.issues.iter().all(|i| i.code != "recognitionDoubt"));
}

#[test]
fn speech_evidence_finds_uncovered_voice_without_flagging_silent_tail() {
    let (a, t) = sample();
    let mut r = check(&a, &t, None, None);
    let speech = serde_json::json!({"speech":[{"start_ms":0,"end_ms":9000},{"start_ms":18000,"end_ms":25000},{"start_ms":30000,"end_ms":34000}]});
    course_workbench_lib::integrity::add_quality_evidence(&mut r, &t, Some(&speech), None).unwrap();
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "uncoveredSpeech" && i.start_ms == 18000 && i.end_ms == 25000));
    assert!(r
        .issues
        .iter()
        .any(|i| i.code == "tail" && i.severity == "info"));
    assert_eq!(r.audio_check.status, "available");
    assert!(r.issues.iter().all(|i| !i.id.is_empty()));
}

#[test]
fn diagnostics_are_suspicions_and_invalid_speech_is_rejected() {
    let (a, t) = sample();
    let mut r = check(&a, &t, None, None);
    let diagnostics = serde_json::json!([{"id":"a","diagnostics":{"avg_logprob":-1.4,"compression_ratio":3.1,"no_speech_prob":0.9}}]);
    course_workbench_lib::integrity::add_quality_evidence(&mut r, &t, None, Some(&diagnostics))
        .unwrap();
    assert!(r.issues.iter().any(|i| i.code == "recognitionDoubt"));
    assert_eq!(r.pending_count, r.issues.len());
    let bad = serde_json::json!({"speech":[{"start_ms":25000,"end_ms":10000}]});
    assert!(
        course_workbench_lib::integrity::add_quality_evidence(&mut r, &t, Some(&bad), None)
            .is_err()
    );
}

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
