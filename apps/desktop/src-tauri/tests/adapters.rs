use course_core::Segment;
use course_workbench_lib::{
    knowledge::{build_payload, validate_endpoint},
    settings::AppSettings,
    source::{classify_subtitles, extract_bvid, parse_view},
};
use serde_json::json;

#[test]
fn collection_url_is_reduced_to_a_single_video_before_any_download() {
    assert_eq!(
        extract_bvid("https://www.bilibili.com/list/ml2572578436?oid=416090103&bvid=BV1JV411t7ow")
            .unwrap(),
        "BV1JV411t7ow"
    );
    assert_eq!(extract_bvid("BV1JV411t7ow").unwrap(), "BV1JV411t7ow");
    assert!(extract_bvid("https://example.com/?bvid=BV1JV411t7ow").is_err());
    assert!(extract_bvid("https://www.bilibili.com.evil.invalid/BV1JV411t7ow").is_err());
}

#[test]
fn subtitle_states_are_not_conflated() {
    assert_eq!(
        classify_subtitles(&json!({"code": -101})).0,
        "loginRequired"
    );
    assert_eq!(classify_subtitles(&json!({"code": -412})).0, "failed");
    assert_eq!(
        classify_subtitles(
            &json!({"code":0,"data":{"need_login_subtitle":true,"subtitle":{"subtitles":[]}}})
        )
        .0,
        "loginRequired"
    );
    assert_eq!(
        classify_subtitles(&json!({"code":0,"data":{"subtitle":{"subtitles":[]}}})).0,
        "absent"
    );
    assert_eq!(classify_subtitles(&json!({"code":0,"data":{"subtitle":{"subtitles":[{"lan":"zh-CN","lan_doc":"中文","subtitle_url":"//aisubtitle.hdslb.com/a.json"}]}}})).0, "available");
    assert_eq!(classify_subtitles(&json!({"code":0})).0, "failed");
}

#[test]
fn preview_preserves_parts_without_automatically_selecting_them() {
    let data = json!({"code":0,"data":{"bvid":"BV1JV411t7ow","title":"课程","pages":[
      {"page":1,"cid":10,"part":"目标","duration":357},
      {"page":2,"cid":11,"part":"导论","duration":1800}]}});
    let preview = parse_view(&data, "BV1JV411t7ow").unwrap();
    assert_eq!(preview.parts.len(), 2);
    assert_eq!(preview.parts[0].duration_ms, 357000);
    assert_eq!(preview.parts[1].subtitle_status, "unchecked");
}

#[test]
fn knowledge_payload_contains_only_the_selected_text_and_question() {
    let settings = AppSettings {
        llm_model: "test-model".into(),
        ..Default::default()
    };
    let segments = vec![Segment {
        id: "s1".into(),
        start_ms: 0,
        end_ms: 1000,
        text: "网络协议".into(),
    }];
    let body = build_payload(&settings, "answer", Some("协议是什么？"), &segments).unwrap();
    let encoded = body.to_string();
    assert!(encoded.contains("网络协议"));
    assert!(encoded.contains("协议是什么"));
    assert!(!encoded.contains("cookieFile"));
    assert!(!encoded.contains("audio_path"));
    assert!(build_payload(&settings, "answer", Some("test"), &[]).is_err());
}

#[test]
fn keys_only_go_to_tls_or_explicit_loopback_endpoints() {
    assert!(validate_endpoint("https://api.example.com/v1").is_ok());
    assert!(validate_endpoint("http://127.0.0.1:8080/v1").is_ok());
    assert!(validate_endpoint("http://api.example.com/v1").is_err());
    assert!(validate_endpoint("https://user:secret@example.com/v1").is_err());
    assert!(validate_endpoint("file:///D:/secrets").is_err());
}
