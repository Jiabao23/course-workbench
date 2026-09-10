//! Conservative completeness checks: evidence of processing, never word accuracy.
use anyhow::{ensure, Result};
use course_core::{Asset, Job, Transcript};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub message: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub note: String,
    pub reviewed_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub transcript_id: String,
    pub version: u32,
    pub status: String,
    pub duration_ms: Option<u64>,
    pub covered_ms: u64,
    pub segment_count: usize,
    pub chunk_done: Option<u32>,
    pub chunk_total: Option<u32>,
    pub issues: Vec<Issue>,
    pub limitations: String,
    pub fingerprint: String,
    pub review: Option<Review>,
}

pub fn require_complete_chunks(done: u32, total: u32, duration_ms: u64) -> Result<()> {
    let expected = duration_ms.div_ceil(300_000);
    ensure!(
        duration_ms > 0
            && total > 0
            && done == total
            && (u64::from(total) == expected
                || (duration_ms.is_multiple_of(300_000) && u64::from(total) == expected + 1)),
        "转录分块未齐全：完成 {done}/{total}，按音频时长应为 {expected} 块。已保留检查点，请重试。"
    );
    Ok(())
}

pub fn check(
    asset: &Asset,
    transcript: &Transcript,
    job: Option<&Job>,
    source_duration: Option<u64>,
) -> IntegrityReport {
    // A standalone caption's end time is not independent evidence of media length.
    let duration =
        (asset.source_kind != "subtitle" && asset.duration_ms > 0).then_some(asset.duration_ms);
    let mut issues = Vec::new();
    let mut add = |code: &str, start_ms: u64, end_ms: u64, message: &str| {
        issues.push(Issue {
            code: code.into(),
            start_ms,
            end_ms,
            message: message.into(),
        })
    };
    if duration.is_none() {
        add(
            "unknownDuration",
            0,
            0,
            "缺少独立音视频时长，无法核对头尾是否齐全。字幕结束时间不能证明课程已结束。",
        );
    }
    if transcript.segments.is_empty() {
        add("empty", 0, 0, "文字稿为空，需要重新处理或核对来源。");
    }
    if matches!(transcript.source_kind.as_str(), "whisper" | "edited") {
        if let Some(job) = job {
            if duration.is_some_and(|d| {
                require_complete_chunks(job.chunk_done, job.chunk_total, d).is_err()
            }) {
                add(
                    "incompleteChunks",
                    0,
                    0,
                    "对应任务的分块证据不齐全，请重试生成新版本。",
                );
            }
        } else {
            add(
                "unknownChunks",
                0,
                0,
                "此版本没有独立的原始任务分块证据；校对版本不会借用其他版本的完成标记。",
            );
        }
    }
    if let (Some(media), Some(source)) = (duration, source_duration.filter(|d| *d > 0)) {
        if media.abs_diff(source) > 5_000.max(source / 50) {
            add(
                "mediaDurationMismatch",
                media.min(source),
                media.max(source),
                "来源时长与本地媒体时长不一致，可能下载截短或来源发生变化，请核对原视频。",
            );
        }
    }
    let mut sorted: Vec<_> = transcript.segments.iter().collect();
    sorted.sort_by_key(|s| (s.start_ms, s.end_ms));
    if transcript
        .segments
        .windows(2)
        .any(|w| w[0].start_ms > w[1].start_ms)
    {
        add("order", 0, 0, "片段顺序与时间轴不一致。");
    }
    let mut end = 0u64;
    let mut covered = 0u64;
    for (index, s) in sorted.iter().enumerate() {
        if s.end_ms <= s.start_ms || s.text.trim().is_empty() {
            add(
                "invalidSegment",
                s.start_ms,
                s.end_ms,
                "片段时间或文字为空，请校对。",
            );
        }
        if index == 0 && s.start_ms > 10_000 {
            add(
                "head",
                0,
                s.start_ms,
                "开头超过 10 秒没有文字；可能是片头或漏转，请回听。",
            );
        }
        if index > 0 && s.start_ms.saturating_sub(end) > 15_000 {
            add(
                "gap",
                end,
                s.start_ms,
                "超过 15 秒没有文字；静音、演示和漏转均可能造成，请回听确认。",
            );
        }
        if index > 0 && end.saturating_sub(s.start_ms) > 2_000 {
            add(
                "overlap",
                s.start_ms,
                end,
                "片段重叠超过 2 秒，请核对重复或时间轴异常。",
            );
        }
        if duration.is_some_and(|d| s.end_ms > d.saturating_add(2_000)) {
            add(
                "outOfBounds",
                s.start_ms,
                s.end_ms,
                "片段超出媒体时长，请核对时间轴。",
            );
        }
        let bounded_end = duration.map_or(s.end_ms, |d| s.end_ms.min(d));
        covered = covered.saturating_add(bounded_end.saturating_sub(s.start_ms.max(end)));
        end = end.max(s.end_ms);
        if index >= 2
            && s.text.trim() == sorted[index - 1].text.trim()
            && s.text.trim() == sorted[index - 2].text.trim()
        {
            add(
                "repetition",
                sorted[index - 2].start_ms,
                s.end_ms,
                "相邻三个片段文字相同；可能为实际重复或模型重复，请回听。",
            );
        }
    }
    if let Some(d) = duration {
        if d.saturating_sub(end) > 10_000.max((d / 50).min(30_000)) {
            add(
                "tail",
                end,
                d,
                "文字早于媒体结束，尾部可能有静音、片尾或漏转，请回听。",
            );
        }
    }
    let status = if issues.is_empty() {
        "noObviousIssues"
    } else {
        "needsReview"
    };
    let mut report=IntegrityReport {transcript_id:transcript.id.clone(),version:transcript.version,status:status.into(),duration_ms:duration,covered_ms:covered,segment_count:transcript.segments.len(),chunk_done:job.map(|j|j.chunk_done),chunk_total:job.map(|j|j.chunk_total),issues,limitations:"规则核对不能证明逐字无遗漏，也不是识别准确率。静音、配乐与字幕间隔可能造成空白；请回听疑点并记录人工结论。".into(),fingerprint:String::new(),review:None};
    // Includes text and independent evidence; acquiring audio invalidates old reviews.
    let bytes = serde_json::to_vec(&(
        &report,
        &transcript.segments,
        source_duration,
        &asset.audio_path,
    ))
    .expect("serializable report");
    report.fingerprint = hex::encode(Sha256::digest(bytes));
    report
}
