use course_core::subtitles::parse_subtitles;

#[test]
fn srt_keeps_milliseconds_multiline_text_and_deterministic_ids() {
    let source = "\u{feff}1\r\n00:00:01,234 --> 00:00:03,678\r\n第一行\r\n<b>第二行</b> & 内容\r\n\r\n2\r\n00:00:04,005 --> 00:00:05,010\r\n你好，世界。\r\n";
    let segments = parse_subtitles(source, "srt").unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!(segments[0].start_ms, 1234);
    assert_eq!(segments[0].end_ms, 3678);
    assert_eq!(segments[0].text, "第一行\n<b>第二行</b> & 内容");
    assert_eq!(segments[1].start_ms, 4005);
    assert_eq!(segments[1].end_ms, 5010);
    assert!(!segments[0].id.is_empty());
    assert_ne!(segments[0].id, segments[1].id);
    assert_eq!(segments, parse_subtitles(source, "SRT").unwrap());
}

#[test]
fn vtt_handles_header_cue_names_notes_and_settings() {
    let source = "WEBVTT - 中文课程\nLanguage: zh\n\nNOTE 这是注释\n不会成为字幕\n\nSTYLE\n::cue { color: white; }\n\nintro\n00:01.001 --> 00:02.999 align:start position:0%\n<v 讲师>多行\n正文\n\n00:00:04.050 --> 00:00:06.070\n下一段\n";
    let segments = parse_subtitles(source, "vtt").unwrap();
    assert_eq!(segments.len(), 2);
    assert_eq!((segments[0].start_ms, segments[0].end_ms), (1001, 2999));
    assert_eq!(segments[0].text, "<v 讲师>多行\n正文");
    assert_eq!((segments[1].start_ms, segments[1].end_ms), (4050, 6070));
}

#[test]
fn bilibili_json_rounds_seconds_to_milliseconds_without_losing_text() {
    let source = r#"{"font_size":0.4,"body":[{"from":0.123,"to":2.456,"content":"变量 x = 1\n不是指令"},{"from":2.4565,"to":3.9995,"content":"完成"}]}"#;
    let segments = parse_subtitles(source, "json").unwrap();
    assert_eq!((segments[0].start_ms, segments[0].end_ms), (123, 2456));
    assert_eq!(segments[0].text, "变量 x = 1\n不是指令");
    assert_eq!((segments[1].start_ms, segments[1].end_ms), (2457, 4000));
    assert_eq!(segments, parse_subtitles(source, "bilibili").unwrap());
}

#[test]
fn overlapping_captions_are_valid_but_ids_are_unique() {
    let source =
        "1\n00:00:01,000 --> 00:00:03,000\n重叠\n\n2\n00:00:01,000 --> 00:00:03,000\n重叠\n";
    let segments = parse_subtitles(source, "srt").unwrap();
    assert_ne!(segments[0].id, segments[1].id);
}

#[test]
fn webvtt_word_in_later_cue_identifier_does_not_discard_the_cue() {
    let segments = parse_subtitles(
        "WEBVTT\n\nWEBVTT lesson\n00:00:01.000 --> 00:00:02.000\n保留这一段\n",
        "vtt",
    )
    .unwrap();
    assert_eq!(segments.len(), 1);
    assert_eq!(segments[0].text, "保留这一段");
}

#[test]
fn rejects_missing_captions_invalid_timing_and_out_of_order_starts() {
    for (source, format) in [
        ("", "srt"),
        ("this is just plain text", "srt"),
        ("WEBVTT\n\nNOTE no cues", "vtt"),
        ("1\n00:99:01,000 --> 00:99:02,000\nwrong minutes", "srt"),
        ("1\n00:00:02,000 --> 00:00:01,000\nreverse", "srt"),
        (
            "1\n00:00:01,000 --> 00:00:02,000 --> 00:00:03,000\nambiguous",
            "srt",
        ),
        ("1\n00:00:01,000 --> 00:00:01,000\nzero length", "srt"),
        ("1\n00:00:01,000 --> 00:00:02,000\n   ", "srt"),
        (
            "1\n00:00:03,000 --> 00:00:04,000\nfirst\n\n2\n00:00:01,000 --> 00:00:02,000\nsecond",
            "srt",
        ),
        (r#"{"body":[]}"#, "json"),
        (
            r#"{"body":[{"from":-1,"to":2,"content":"negative"}]}"#,
            "json",
        ),
        (
            r#"{"body":[{"from":0,"to":1e30,"content":"overflow"}]}"#,
            "json",
        ),
        (r#"{"body":[{"from":0,"to":1,"content":""}]}"#, "json"),
        ("subtitle", "ass"),
    ] {
        assert!(
            parse_subtitles(source, format).is_err(),
            "accepted {format}: {source}"
        );
    }
}
