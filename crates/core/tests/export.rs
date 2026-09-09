mod common;

use common::{segment, transcript};
use course_core::{export::export_transcript, subtitles::parse_subtitles};

#[test]
fn timed_exports_round_trip_multiline_captions_and_large_hours_exactly() {
    let original = transcript(vec![
        segment("a", 1001, 2999, "第一行\n第二行"),
        segment("b", 90_000_010, 90_001_234, "超过一天的课程"),
    ]);
    for format in ["srt", "vtt"] {
        let exported = export_transcript(&original, "标题", None, format).unwrap();
        let parsed = parse_subtitles(&exported, format).unwrap();
        assert_eq!(parsed.len(), original.segments.len());
        for (actual, expected) in parsed.iter().zip(&original.segments) {
            assert_eq!(
                (actual.start_ms, actual.end_ms),
                (expected.start_ms, expected.end_ms)
            );
            assert_eq!(actual.text, expected.text);
        }
        assert!(exported.contains(if format == "srt" {
            "25:00:00,010"
        } else {
            "25:00:00.010"
        }));
    }
}

#[test]
fn readable_exports_include_title_source_and_preserved_content() {
    let original = transcript(vec![segment("a", 1234, 5678, "温度 < 30°C\n原始多行内容")]);
    for format in ["txt", "md"] {
        let exported = export_transcript(
            &original,
            "热力学课程",
            Some("https://example.com/course"),
            format,
        )
        .unwrap();
        assert!(exported.contains("热力学课程"));
        assert!(exported.contains("https://example.com/course"));
        assert!(exported.contains("温度 < 30°C\n原始多行内容"));
    }
}

#[test]
fn export_rejects_unknown_formats_and_invalid_ranges() {
    let valid = transcript(vec![segment("a", 1, 10, "字幕")]);
    assert!(export_transcript(&valid, "title", None, "html").is_err());
    let invalid = transcript(vec![segment("a", 10, 1, "字幕")]);
    assert!(export_transcript(&invalid, "title", None, "srt").is_err());
}

#[test]
fn timed_exports_normalize_blank_cue_lines_but_readable_exports_keep_original() {
    let original = transcript(vec![segment("a", 1000, 3000, "第一段\n\n  \n第二段")]);
    for format in ["srt", "vtt"] {
        let exported = export_transcript(&original, "标题", None, format).unwrap();
        let reparsed = parse_subtitles(&exported, format).unwrap();
        assert_eq!(reparsed.len(), 1);
        assert_eq!(reparsed[0].text, "第一段\n第二段");
        assert_eq!((reparsed[0].start_ms, reparsed[0].end_ms), (1000, 3000));
    }
    for format in ["txt", "md"] {
        assert!(export_transcript(&original, "标题", None, format)
            .unwrap()
            .contains(&original.segments[0].text));
    }
}

#[test]
fn timed_exports_normalize_bare_carriage_returns() {
    let original = transcript(vec![segment("a", 1, 999, "第一段\r\r第二段")]);
    for format in ["srt", "vtt"] {
        let exported = export_transcript(&original, "标题", None, format).unwrap();
        let parsed = parse_subtitles(&exported, format).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].text, "第一段\n第二段");
    }
}

#[test]
fn vtt_payload_escapes_timing_arrows_without_changing_stored_text() {
    let original = transcript(vec![segment("a", 1, 999, "left --> right")]);
    let exported = export_transcript(&original, "标题", None, "vtt").unwrap();
    assert!(exported.contains("left --&gt; right"));
    assert_eq!(
        parse_subtitles(&exported, "vtt").unwrap()[0].text,
        "left --> right"
    );
    assert_eq!(original.segments[0].text, "left --> right");
}

#[test]
fn markdown_has_safe_stable_targets_for_local_note_citations() {
    let original = transcript(vec![segment("lesson-001", 1000, 2000, "可引用的原文")]);
    let output = export_transcript(&original, "课程", None, "md").unwrap();
    assert!(output.contains("<a id=\"segment-lesson-001\"></a>"));
    let hostile = transcript(vec![segment("a\"<b>&", 1000, 2000, "原文")]);
    let output = export_transcript(&hostile, "课程", None, "md").unwrap();
    assert!(output.contains("id=\"segment-a&quot;&lt;b&gt;&amp;\""));
}
