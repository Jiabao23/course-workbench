use course_workbench_lib::{
    cache,
    service::{CreateJobsRequest, Runtime},
    settings::{self, AppSettings},
};
use std::{
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
use tempfile::TempDir;

fn runtime() -> (TempDir, std::sync::Arc<Runtime>, PathBuf) {
    let temp = TempDir::new().unwrap();
    let settings = AppSettings {
        data_dir: temp.path().join("library").to_string_lossy().into_owned(),
        model_dir: temp
            .path()
            .join("external-models")
            .to_string_lossy()
            .into_owned(),
        python_path: String::new(),
        ffmpeg_path: String::new(),
        ffprobe_path: String::new(),
        yt_dlp_path: String::new(),
        preset: "custom".into(),
        model: "tiny".into(),
        device: "cpu".into(),
        ..Default::default()
    };
    let config = temp.path().join("settings.json");
    settings::save(&config, &settings).unwrap();
    let source = temp.path().join("lesson.srt");
    fs::write(&source,"1\n00:00:01,250 --> 00:00:03,750\n计算机网络采用分层体系结构。\n\n2\n00:00:04,000 --> 00:00:06,000\n协议规定通信双方的行为。\n").unwrap();
    let app = Runtime::new(config, temp.path().join("intentionally-missing-worker.py")).unwrap();
    (temp, app, source)
}

fn imported(app: &std::sync::Arc<Runtime>, source: &std::path::Path) -> course_core::Asset {
    let jobs = app
        .create_jobs(CreateJobsRequest {
            source: source.to_string_lossy().into_owned(),
            pages: vec![1],
            mode: "auto".into(),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let job = app.db().get_job(&jobs[0].id).unwrap();
        if job.status == "completed" && !app.is_busy() {
            return app.db().get_asset(&job.asset_id).unwrap();
        }
        assert_ne!(job.status, "failed", "{:?}", job.error);
        assert!(Instant::now() < deadline, "job did not finish");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn subtitle_path_completes_without_python_ffmpeg_audio_or_asr() {
    let (_temp, app, source) = runtime();
    let asset = imported(&app, &source);
    assert!(asset.audio_path.is_none());
    assert!(fs::read_dir(app.settings().cache_path("audio"))
        .unwrap()
        .next()
        .is_none());
    assert!(fs::read_dir(app.settings().cache_path("checkpoints"))
        .unwrap()
        .next()
        .is_none());
    let detail = app.asset_detail(&asset.id).unwrap();
    assert_eq!(detail.transcript.unwrap().segments.len(), 2);
    assert!(!app.search("分层", Some(&asset.id)).unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn reimporting_legacy_windows_paths_preserves_asset_versions_and_notes() {
    let (_temp, app, source) = runtime();
    let mut asset = imported(&app, &source);
    let original = app.asset_detail(&asset.id).unwrap().transcript.unwrap();
    let note = app
        .save_manual_note(
            &asset.id,
            &original.id,
            "旧版笔记",
            "保留原来的出处。",
            &[original.segments[0].id.clone()],
        )
        .unwrap();
    // Version 0.1.0 persisted std::fs::canonicalize's extended-length path.
    asset.source = source
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(asset.source.starts_with(r"\\?\"));
    app.db().upsert_asset(&asset).unwrap();

    let imported_again = imported(&app, &source);
    assert_eq!(
        imported_again.id, asset.id,
        "reimport must reuse the existing local asset"
    );
    assert_eq!(app.db().list_assets().unwrap().len(), 1);
    let detail = app.asset_detail(&asset.id).unwrap();
    assert_eq!(detail.versions.len(), 2);
    assert_eq!(detail.notes[0].id, note.id);
    assert_eq!(detail.notes[0].transcript_id, original.id);
    assert!(detail.notes[0].stale);
}

#[test]
fn playback_paths_resolve_parent_components_and_hide_missing_audio() {
    let (_temp, app, source) = runtime();
    let mut asset = imported(&app, &source);
    let audio = app.settings().cache_path("audio").join("lesson.wav");
    fs::write(&audio, b"cached audio").unwrap();
    let legacy_path = app
        .settings()
        .cache_path("audio")
        .join("..")
        .join("audio/lesson.wav");
    asset.audio_path = Some(legacy_path.to_string_lossy().into_owned());
    app.db().upsert_asset(&asset).unwrap();

    let exposed = app.asset_detail(&asset.id).unwrap();
    let playback = PathBuf::from(exposed.asset.audio_path.unwrap());
    assert_eq!(playback, dunce::canonicalize(&audio).unwrap());
    assert!(tauri::path::SafePathBuf::new(playback).is_ok());

    fs::remove_file(&audio).unwrap();
    let missing = app.asset_detail(&asset.id).unwrap();
    assert!(missing.asset.audio_path.is_none());
    assert!(missing.transcript.is_some());
    assert_eq!(
        app.db().get_asset(&asset.id).unwrap().audio_path,
        asset.audio_path
    );
}

#[test]
fn web_sources_report_missing_downloader_without_using_the_bilibili_adapter() {
    let (_temp, app, _source) = runtime();
    let error = match app.probe_source("https://www.youtube.com/watch?v=public-video") {
        Ok(_) => panic!("no downloader was configured"),
        Err(error) => format!("{error:#}"),
    };
    assert!(error.contains("yt-dlp"), "{error}");
    assert!(!error.contains("仅支持 bilibili"), "{error}");
}

#[test]
fn second_runtime_cannot_recover_a_library_that_is_still_open() {
    let (temp, app, source) = runtime();
    let _asset = imported(&app, &source);
    let mut job = app.db().list_jobs().unwrap()[0].clone();
    job.status = "running".into();
    app.db().upsert_job(&job).unwrap();
    assert!(Runtime::new(app.config_path.clone(), app.worker_path.clone()).is_err());
    assert_eq!(app.db().get_job(&job.id).unwrap().status, "running");
    let config = app.config_path.clone();
    drop(app);
    let reopened = Runtime::new(config, temp.path().join("missing-worker.py")).unwrap();
    assert_eq!(reopened.db().get_job(&job.id).unwrap().status, "paused");
}

#[test]
fn edit_note_export_and_old_version_citations_remain_consistent() {
    let (temp, app, source) = runtime();
    let asset = imported(&app, &source);
    let original = app.asset_detail(&asset.id).unwrap().transcript.unwrap();
    let note = app
        .save_manual_note(
            &asset.id,
            &original.id,
            "分层笔记",
            "需要掌握每一层的职责。",
            &[original.segments[0].id.clone()],
        )
        .unwrap();
    let mut edited = original.segments.clone();
    edited[0].text = "网络使用分层体系结构和协议。".into();
    let new = app.save_edit(&asset.id, &original.id, &edited).unwrap();
    assert_eq!(new.version, 2);
    assert!(app.save_edit(&asset.id, &original.id, &edited).is_err());
    let detail = app.asset_detail(&asset.id).unwrap();
    assert!(detail.notes[0].stale);
    assert_eq!(detail.notes[0].citations, note.citations);
    let output = temp.path().join("export.md");
    app.export(
        &asset.id,
        "md",
        output.to_str().unwrap(),
        Some(&original.id),
    )
    .unwrap();
    let text = fs::read_to_string(output).unwrap();
    assert!(text.contains("分层笔记"));
    assert!(text.contains("计算机网络采用分层体系结构"));
    app.activate_version(&asset.id, &original.id).unwrap();
    assert!(!app.asset_detail(&asset.id).unwrap().notes[0].stale);
}

#[test]
fn clearing_owned_cache_preserves_shared_models_originals_and_database() {
    let (_temp, app, source) = runtime();
    let asset = imported(&app, &source);
    let settings = app.settings();
    fs::create_dir_all(&settings.model_dir).unwrap();
    let model = PathBuf::from(&settings.model_dir).join("shared.pt");
    fs::write(&model, b"existing model").unwrap();
    fs::write(settings.cache_path("audio").join("derived.wav"), b"cache").unwrap();
    assert!(
        !cache::inventory(&settings)
            .unwrap()
            .iter()
            .find(|c| c.id == "models")
            .unwrap()
            .clearable
    );
    assert!(cache::clear(&settings, "models").is_err());
    assert!(cache::clear(&settings, "../library.sqlite3").is_err());
    cache::clear(&settings, "audio").unwrap();
    assert!(source.is_file());
    assert!(model.is_file());
    assert!(app.db().active_transcript(&asset.id).unwrap().is_some());
}

#[test]
fn switching_libraries_recovers_orphaned_jobs_before_enabling_actions() {
    let (temp, app, source) = runtime();
    let asset = imported(&app, &source);
    let mut interrupted = app.db().list_jobs().unwrap()[0].clone();
    interrupted.status = "running".into();
    app.db().upsert_job(&interrupted).unwrap();
    let original = app.settings();
    drop(app);
    // Simulate a different library reopened after a prior process died.
    let other_config = temp.path().join("other-settings.json");
    let other = AppSettings {
        data_dir: temp
            .path()
            .join("other-library")
            .to_string_lossy()
            .into_owned(),
        ..original.clone()
    };
    settings::save(&other_config, &other).unwrap();
    let other_app = Runtime::new(other_config, temp.path().join("no-worker.py")).unwrap();
    other_app.save_settings(original).unwrap();
    assert_eq!(
        other_app.db().get_job(&interrupted.id).unwrap().status,
        "paused"
    );
    assert_eq!(
        other_app.cancel_job(&interrupted.id).unwrap().status,
        "cancelled"
    );
    assert!(other_app
        .db()
        .active_transcript(&asset.id)
        .unwrap()
        .is_some());
}

#[test]
fn cancellation_before_commit_leaves_no_version_and_can_retry() {
    let (_temp, app, source) = runtime();
    let (send, receive) = std::sync::mpsc::channel();
    let signal = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let waiting = signal.clone();
    app.set_event_sink(std::sync::Arc::new(move |job| {
        if job.stage == "读取字幕" && !*waiting.0.lock().unwrap() {
            send.send(job.id).unwrap();
            let mut released = waiting.0.lock().unwrap();
            while !*released {
                released = waiting.1.wait(released).unwrap();
            }
        }
    }));
    let jobs = app
        .create_jobs(CreateJobsRequest {
            source: source.to_string_lossy().into_owned(),
            pages: vec![1],
            mode: "auto".into(),
        })
        .unwrap();
    let id = receive.recv_timeout(Duration::from_secs(15)).unwrap();
    app.cancel_job(&id).unwrap();
    *signal.0.lock().unwrap() = true;
    signal.1.notify_all();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.db().get_job(&id).unwrap().status != "cancelled" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(app
        .db()
        .active_transcript(&jobs[0].asset_id)
        .unwrap()
        .is_none());
    app.retry_job(&id, false).unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.db().get_job(&id).unwrap().status != "completed" {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        app.db().list_transcripts(&jobs[0].asset_id).unwrap().len(),
        1
    );
}

#[cfg(windows)]
#[test]
fn cache_root_junction_cannot_delete_originals_inside_the_library() {
    let (_temp, app, _) = runtime();
    let settings = app.settings();
    let original = settings.data_path().join("originals");
    fs::create_dir_all(&original).unwrap();
    fs::write(original.join("keep.txt"), b"original").unwrap();
    let link = settings.cache_path("audio");
    fs::remove_dir(&link).unwrap();
    let status=std::process::Command::new("powershell.exe").args(["-NoProfile","-NonInteractive","-Command","New-Item -ItemType Junction -Path $env:CW_TEST_LINK -Target $env:CW_TEST_TARGET | Out-Null"])
        .env("CW_TEST_LINK",&link).env("CW_TEST_TARGET",&original).status().unwrap();
    assert!(status.success());
    let cleared = cache::clear(&settings, "audio");
    fs::remove_dir(&link).unwrap();
    assert!(cleared.is_err());
    assert_eq!(fs::read(original.join("keep.txt")).unwrap(), b"original");
}
