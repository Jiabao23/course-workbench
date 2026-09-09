use course_workbench_lib::{
    settings::AppSettings,
    source::{is_bilibili_source, local_preview, SubtitleTrack},
    web_source::{
        extractor_error, metadata_command, parse_metadata, subtitle_command, validate_web_url,
    },
};
use serde_json::json;
use std::fs;
use tempfile::TempDir;

fn settings() -> AppSettings {
    AppSettings {
        yt_dlp_path: "yt-dlp.exe".into(),
        python_path: String::new(),
        ffmpeg_path: String::new(),
        cookie_file: String::new(),
        ..Default::default()
    }
}

#[test]
fn routes_domains_without_treating_other_sites_as_bilibili() {
    assert!(is_bilibili_source("BV1JV411t7ow"));
    assert!(is_bilibili_source(
        "https://www.bilibili.com/video/BV1JV411t7ow"
    ));
    assert!(!is_bilibili_source(
        "https://bilibili.com.evil.example/video/BV1JV411t7ow"
    ));
    assert!(!is_bilibili_source(
        "https://www.youtube.com/watch?v=sample"
    ));
    assert!(validate_web_url("https://media.example.org/sample.mp4?token=a%20b").is_ok());
    for source in [
        "file:///D:/private.wav",
        "ftp://example.org/audio",
        "https://user:pass@example.org/video",
    ] {
        assert!(validate_web_url(source).is_err());
    }
}

#[test]
fn normalizes_one_video_and_prioritizes_readable_manual_captions() {
    let preview = parse_metadata(&json!({
        "title":"示例课程", "duration":70.25,
        "webpage_url":"https://example.org/video/1",
        "formats":[{"vcodec":"none","acodec":"aac"}],
        "subtitles":{
            "en":[{"ext":"vtt","url":"https://example.org/en.vtt"}],
            "zh-Hans":[{"ext":"json3","url":"https://example.org/a.json"},{"ext":"vtt","url":"https://example.org/zh.vtt"}]
        },
        "automatic_captions":{"zh-Hans":[{"ext":"vtt","url":"https://example.org/auto.vtt"}]}
    }), "https://example.org/original", "zh").unwrap();
    assert_eq!(preview.source_kind, "webMedia");
    assert_eq!(preview.source, "https://example.org/video/1");
    assert!(preview.bvid.is_none());
    assert_eq!(preview.parts.len(), 1);
    assert_eq!(preview.parts[0].duration_ms, 70250);
    assert_eq!(preview.parts[0].subtitle_status, "available");
    assert_eq!(preview.parts[0].subtitles.len(), 2);
    assert_eq!(
        preview.parts[0].subtitles[0].url,
        "https://example.org/zh.vtt"
    );
    assert!(preview.parts[0].subtitles[0].label.contains("人工字幕"));
    assert!(preview.warnings.is_empty());
}

#[test]
fn automatic_unreadable_absent_and_login_captions_have_explicit_results() {
    let automatic = parse_metadata(&json!({"title":"lesson","automatic_captions":{"zh":[{"ext":"srt","url":"https://example.org/auto.srt"}]}}), "https://example.org/video", "zh").unwrap();
    assert!(automatic.parts[0].subtitles[0].label.contains("自动字幕"));
    let unsupported = parse_metadata(&json!({"title":"lesson","subtitles":{"zh":[{"ext":"json3","url":"https://example.org/caption"}]}}), "https://example.org/video", "zh").unwrap();
    assert_eq!(unsupported.parts[0].subtitle_status, "absent");
    assert!(unsupported
        .warnings
        .iter()
        .any(|warning| warning.contains("SRT/VTT")));
    let absent = parse_metadata(
        &json!({"title":"lesson"}),
        "https://example.org/video",
        "zh",
    )
    .unwrap();
    assert_eq!(absent.parts[0].subtitle_status, "absent");
    let login = parse_metadata(
        &json!({"title":"lesson","availability":"needs_auth"}),
        "https://example.org/video",
        "zh",
    )
    .unwrap();
    assert_eq!(login.parts[0].subtitle_status, "loginRequired");
}

#[test]
fn playlists_live_streams_and_protected_sources_are_rejected() {
    for value in [
        json!({"_type":"playlist","title":"list","entries":[]}),
        json!({"_type":"multi_video","title":"list"}),
        json!({"title":"live","is_live":true}),
        json!({"title":"scheduled","live_status":"is_upcoming"}),
        json!({"title":"protected","has_drm":true}),
    ] {
        assert!(parse_metadata(&value, "https://example.org/video", "zh").is_err());
    }
}

#[test]
fn mixed_media_fallback_is_disclosed_before_submission() {
    let preview = parse_metadata(
        &json!({"title":"mp4","duration":12.0,"formats":[{"vcodec":"h264","acodec":"aac"}]}),
        "https://example.org/sample.mp4",
        "zh",
    )
    .unwrap();
    assert!(preview
        .warnings
        .iter()
        .any(|warning| warning.contains("下载原媒体")));
}

#[test]
fn metadata_command_prevents_media_download_and_bounds_playlists() {
    let source = "https://example.org/video?id=1&list=course";
    let command = metadata_command(&settings(), source).unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"--skip-download".to_owned()));
    assert!(args.contains(&"--no-playlist".to_owned()));
    assert!(args.contains(&"--flat-playlist".to_owned()));
    assert!(args.windows(2).any(|pair| pair == ["--playlist-end", "1"]));
    assert_eq!(&args[args.len() - 2..], &["--", source]);
}

#[test]
fn subtitle_command_only_downloads_the_explicit_language_without_shell_interpolation() {
    let temp = TempDir::new().unwrap();
    let directory = temp.path().join("课程 文件");
    let track = SubtitleTrack {
        language: "zh.*".into(),
        label: "测试".into(),
        url: "https://example.org/caption".into(),
        format: "vtt".into(),
        automatic: false,
    };
    let command = subtitle_command(
        &settings(),
        "https://example.org/video?a=1&b=2",
        &track,
        &directory,
    )
    .unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    assert!(args.contains(&"--skip-download".to_owned()));
    assert!(args.contains(&"--write-subs".to_owned()));
    assert!(args
        .windows(2)
        .any(|pair| pair == ["--sub-langs", "^zh\\.\\*$"]));
    assert!(args.contains(
        &directory
            .join("subtitle.%(ext)s")
            .to_string_lossy()
            .into_owned()
    ));
    assert!(!args.contains(&"--extract-audio".to_owned()));
}

#[test]
fn extractor_failures_remain_errors_instead_of_becoming_absent_subtitles() {
    assert!(extractor_error("Sign in to confirm your age").contains("登录"));
    assert!(extractor_error("Unsupported URL").contains("更新下载工具"));
    assert!(extractor_error("HTTP 429: Too Many Requests").contains("限制了请求"));
    assert!(extractor_error("connection timed out").contains("解析失败"));
}

#[test]
fn readable_automatic_captions_are_not_shadowed_by_unreadable_manual_captions() {
    let preview = parse_metadata(
        &json!({
            "title":"lesson",
            "subtitles":{"zh":[{"ext":"json3","url":"https://example.org/manual.json"}]},
            "automatic_captions":{"zh":[{"ext":"vtt","url":"https://example.org/auto.vtt"}]}
        }),
        "https://example.org/video",
        "zh",
    )
    .unwrap();
    let track = &preview.parts[0].subtitles[0];
    assert!(track.label.contains("自动字幕"));
    let directory = TempDir::new().unwrap();
    let command = subtitle_command(&settings(), &preview.source, track, directory.path()).unwrap();
    let args: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect();
    assert!(
        args.contains(&"--no-write-subs".to_owned()),
        "manual captions would take priority over the selected automatic track"
    );
    assert!(args.contains(&"--write-auto-subs".to_owned()));
}

#[test]
fn local_upload_accepts_unicode_paths_and_rejects_missing_or_invalid_files() {
    let temp = TempDir::new().unwrap();
    let subtitle = temp.path().join("第一节 字幕.SRT");
    fs::write(
        &subtitle,
        "1\n00:00:01,000 --> 00:00:03,500\n本地文件导入。\n",
    )
    .unwrap();
    let preview = local_preview(subtitle.to_str().unwrap()).unwrap();
    assert_eq!(preview.source_kind, "subtitle");
    assert_eq!(preview.parts[0].duration_ms, 3500);
    assert_eq!(
        preview.source,
        dunce::canonicalize(&subtitle).unwrap().to_string_lossy()
    );
    assert!(local_preview(temp.path().to_str().unwrap()).is_err());
    assert!(local_preview(temp.path().join("missing.mp4").to_str().unwrap()).is_err());
    fs::write(&subtitle, "this is not a subtitle").unwrap();
    assert!(local_preview(subtitle.to_str().unwrap()).is_err());
}
