#![allow(dead_code)]

use course_core::types::{Asset, Job, Segment, Transcript};

pub fn segment(id: &str, start_ms: u64, end_ms: u64, text: &str) -> Segment {
    Segment {
        id: id.into(),
        start_ms,
        end_ms,
        text: text.into(),
    }
}

pub fn asset(id: &str) -> Asset {
    Asset {
        id: id.into(),
        title: format!("课程 {id}"),
        source_kind: "local".into(),
        source: format!("{id}.mp4"),
        bvid: None,
        page: None,
        duration_ms: 120_000,
        audio_path: None,
        active_version_id: None,
        created_at: "2026-09-09T00:00:00Z".into(),
        updated_at: "2026-09-09T00:00:00Z".into(),
    }
}

pub fn transcript(segments: Vec<Segment>) -> Transcript {
    Transcript {
        id: "version-1".into(),
        asset_id: "asset-1".into(),
        version: 1,
        source_kind: "subtitle".into(),
        model: None,
        language: "zh".into(),
        segments,
        created_at: "2026-09-09T00:00:00Z".into(),
        is_active: true,
    }
}

pub fn job(id: &str, asset_id: &str, status: &str) -> Job {
    Job {
        id: id.into(),
        asset_id: asset_id.into(),
        title: "转写课程".into(),
        status: status.into(),
        stage: "transcribing".into(),
        progress: 40.0,
        error: None,
        model: "small".into(),
        device: "cuda".into(),
        mode: "transcribe".into(),
        preset: "balanced".into(),
        chunk_done: 2,
        chunk_total: 5,
        created_at: "2026-09-09T00:00:00Z".into(),
        updated_at: "2026-09-09T00:00:00Z".into(),
    }
}
