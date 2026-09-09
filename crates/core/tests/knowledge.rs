mod common;

use common::segment;
use course_core::knowledge::validate_knowledge_response;

#[test]
fn fenced_json_binds_citations_to_actual_source_text_and_times() {
    let segments = vec![
        segment("s-1", 1234, 5678, "原始证据"),
        segment("s-2", 6000, 9000, "更多证据"),
    ];
    let raw =
        "```json\n{\"content\":\"## 结论\\n文字\",\"citations\":[\"s-2\",\"s-1\",\"s-2\"]}\n```";
    let (content, citations) = validate_knowledge_response(raw, &segments).unwrap();
    assert_eq!(content, "## 结论\n文字");
    assert_eq!(citations.len(), 2);
    assert_eq!(citations[0].segment_id, "s-2");
    assert_eq!(citations[0].text, "更多证据");
    assert_eq!((citations[1].start_ms, citations[1].end_ms), (1234, 5678));
    assert_eq!(citations[1].text, "原始证据");
}

#[test]
fn refuses_invented_or_missing_evidence_and_non_json_answers() {
    let segments = vec![segment("real", 0, 1000, "事实")];
    for raw in [
        r#"{"content":"结论","citations":["invented"]}"#,
        r#"{"content":"结论","citations":[]}"#,
        r#"{"content":"结论"}"#,
        r#"{"content":" ","citations":["real"]}"#,
        r#"{"content":"结论","citations":[{"segmentId":"real"}]}"#,
        "a fluent but unsupported answer",
        "prefix ```json\n{\"content\":\"a\",\"citations\":[\"real\"]}\n``` suffix",
    ] {
        assert!(
            validate_knowledge_response(raw, &segments).is_err(),
            "accepted {raw}"
        );
    }
}

#[test]
fn explicit_insufficiency_returns_a_fixed_message_without_unverified_claims() {
    let (content, citations) = validate_knowledge_response(
        r#"{"content":"模型附加的无依据结论","citations":[],"insufficientEvidence":true}"#,
        &[],
    )
    .unwrap();
    assert!(content.contains("证据不足"));
    assert!(!content.contains("无依据结论"));
    assert!(citations.is_empty());
    assert!(validate_knowledge_response(
        r#"{"insufficientEvidence":true,"citations":["invented"]}"#,
        &[],
    )
    .is_err());
}

#[test]
fn rejects_ambiguous_or_empty_source_segments() {
    let raw = r#"{"content":"结论","citations":["same"]}"#;
    assert!(validate_knowledge_response(
        raw,
        &[
            segment("same", 0, 1000, "一"),
            segment("same", 1000, 2000, "二")
        ]
    )
    .is_err());
    assert!(validate_knowledge_response(raw, &[segment("same", 0, 1000, " ")]).is_err());
}

#[test]
fn inline_markers_must_match_selected_and_declared_citations() {
    let segments = vec![
        segment("real", 0, 1000, "事实"),
        segment("other", 1000, 2000, "更多事实"),
    ];
    for raw in [
        r#"{"content":"结论 [引用:invented]","citations":["real"]}"#,
        r#"{"content":"结论 [引用:other]","citations":["real"]}"#,
        r#"{"content":"结论 [引用:]","citations":["real"]}"#,
        r#"{"content":"结论 [引用:real","citations":["real"]}"#,
        r#"{"content":"[引用:invented]","citations":[],"insufficientEvidence":true}"#,
    ] {
        assert!(
            validate_knowledge_response(raw, &segments).is_err(),
            "accepted {raw}"
        );
    }
}

#[test]
fn valid_inline_markers_preserve_markdown_and_deduplicate_source_citations() {
    let segments = vec![segment("real", 0, 1000, "事实")];
    let raw = r###"{"content":"## 结论\n事实。[引用:real]\n补充。[引用:real]","citations":["real","real"]}"###;
    let (content, citations) = validate_knowledge_response(raw, &segments).unwrap();
    assert_eq!(content, "## 结论\n事实。[引用:real]\n补充。[引用:real]");
    assert_eq!(citations.len(), 1);
    assert_eq!(citations[0].segment_id, "real");
}
