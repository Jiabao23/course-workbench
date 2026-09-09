use crate::types::Segment;
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use uuid::Uuid;

/// Parse caption data without interpreting markup. WebVTT named character
/// references decode to their plain-text value; raw files remain with assets.
pub fn parse_subtitles(text: &str, format: &str) -> Result<Vec<Segment>> {
    let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut segments = match format.as_str() {
        "srt" => parse_timed_text(text, false)?,
        "vtt" | "webvtt" => parse_timed_text(text, true)?,
        "json" | "bilibili" | "bilibili-json" => parse_bilibili(text)?,
        _ => bail!("不支持的字幕格式：{format}"),
    };
    for (index, segment) in segments.iter_mut().enumerate() {
        // Timing, text and occurrence identify a cue deterministically. Edited
        // versions retain these supplied IDs rather than parsing them anew.
        let key = format!(
            "course-workbench/cue/v1/{index}/{}/{}/{}",
            segment.start_ms, segment.end_ms, segment.text
        );
        segment.id = Uuid::new_v5(&Uuid::NAMESPACE_URL, key.as_bytes()).to_string();
    }
    validate_segments(&segments)?;
    Ok(segments)
}

fn parse_timed_text(text: &str, is_vtt: bool) -> Result<Vec<Segment>> {
    let normalized = text
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut block = Vec::new();
    for line in normalized.lines() {
        if line.trim().is_empty() {
            if !block.is_empty() {
                blocks.push(std::mem::take(&mut block));
            }
        } else {
            block.push(line);
        }
    }
    if !block.is_empty() {
        blocks.push(block);
    }

    let mut segments = Vec::new();
    for (block_index, block) in blocks.into_iter().enumerate() {
        let first = block[0].trim();
        if is_vtt
            && ((block_index == 0
                && (first == "WEBVTT"
                    || first.starts_with("WEBVTT ")
                    || first.starts_with("WEBVTT\t")))
                || first == "NOTE"
                || first.starts_with("NOTE ")
                || first.starts_with("NOTE\t")
                || first == "STYLE"
                || first == "REGION")
        {
            continue;
        }
        let timing_index = if first.contains("-->") {
            0
        } else if block.len() > 1 && block[1].contains("-->") {
            ensure!(
                is_vtt || first.chars().all(|character| character.is_ascii_digit()),
                "SRT 字幕序号无效：{first}"
            );
            1
        } else {
            bail!("字幕块缺少有效时间行：{first}");
        };
        let (start_ms, end_ms) = parse_timing_line(block[timing_index])?;
        let text = block[timing_index + 1..].join("\n");
        let text = if is_vtt {
            text.replace("&gt;", ">")
                .replace("&lt;", "<")
                .replace("&nbsp;", "\u{00a0}")
                .replace("&lrm;", "\u{200e}")
                .replace("&rlm;", "\u{200f}")
                .replace("&amp;", "&")
        } else {
            text
        };
        ensure!(!text.trim().is_empty(), "时间行后缺少字幕正文");
        segments.push(Segment {
            id: String::new(),
            start_ms,
            end_ms,
            text,
        });
    }
    Ok(segments)
}

fn parse_timing_line(line: &str) -> Result<(u64, u64)> {
    let (start, rest) = line.split_once("-->").context("缺少字幕时间分隔符")?;
    ensure!(!rest.contains("-->"), "字幕时间行包含多个时间分隔符");
    let end = rest.split_whitespace().next().context("缺少字幕结束时间")?;
    let start_ms = parse_timestamp(start.trim())?;
    let end_ms = parse_timestamp(end)?;
    ensure!(end_ms > start_ms, "字幕结束时间必须晚于开始时间");
    Ok((start_ms, end_ms))
}

fn parse_timestamp(timestamp: &str) -> Result<u64> {
    let parts: Vec<_> = timestamp.split(':').collect();
    ensure!(matches!(parts.len(), 2 | 3), "无效的字幕时间：{timestamp}");
    let hours = if parts.len() == 3 {
        parse_digits(parts[0])?
    } else {
        0
    };
    let minutes = parse_digits(parts[parts.len() - 2])?;
    let (seconds, fraction) = parts[parts.len() - 1]
        .split_once(['.', ','])
        .context("字幕时间缺少毫秒")?;
    let seconds = parse_digits(seconds)?;
    ensure!(minutes < 60 && seconds < 60, "字幕时间的分钟或秒超出范围");
    ensure!(
        (1..=3).contains(&fraction.len()),
        "字幕毫秒必须是 1 至 3 位数字"
    );
    let fraction_ms = parse_digits(fraction)? * 10_u64.pow(3 - fraction.len() as u32);
    hours
        .checked_mul(3_600_000)
        .and_then(|value| value.checked_add(minutes * 60_000))
        .and_then(|value| value.checked_add(seconds * 1000))
        .and_then(|value| value.checked_add(fraction_ms))
        .context("字幕时间超出可支持的范围")
}

fn parse_digits(value: &str) -> Result<u64> {
    ensure!(
        !value.is_empty() && value.chars().all(|character| character.is_ascii_digit()),
        "字幕时间包含非数字字符"
    );
    value.parse().context("字幕时间超出可支持的范围")
}

#[derive(Deserialize)]
struct BilibiliSubtitles {
    body: Vec<BilibiliCue>,
}

#[derive(Deserialize)]
struct BilibiliCue {
    from: f64,
    to: f64,
    content: String,
}

fn parse_bilibili(text: &str) -> Result<Vec<Segment>> {
    let subtitles: BilibiliSubtitles = serde_json::from_str(text.trim_start_matches('\u{feff}'))
        .context("无效的 Bilibili 字幕 JSON")?;
    subtitles
        .body
        .into_iter()
        .map(|cue| {
            Ok(Segment {
                id: String::new(),
                start_ms: seconds_to_ms(cue.from)?,
                end_ms: seconds_to_ms(cue.to)?,
                text: cue.content,
            })
        })
        .collect()
}

fn seconds_to_ms(seconds: f64) -> Result<u64> {
    ensure!(
        seconds.is_finite() && seconds >= 0.0,
        "字幕时间必须是非负有限数值"
    );
    let milliseconds = (seconds * 1000.0).round();
    ensure!(milliseconds < u64::MAX as f64, "字幕时间超出可支持的范围");
    Ok(milliseconds as u64)
}

pub(crate) fn validate_segments(segments: &[Segment]) -> Result<()> {
    ensure!(!segments.is_empty(), "没有可用的字幕内容");
    let mut ids = HashSet::with_capacity(segments.len());
    let mut previous_start = 0;
    for segment in segments {
        ensure!(!segment.id.trim().is_empty(), "字幕片段 ID 不能为空");
        ensure!(ids.insert(&segment.id), "字幕片段 ID 重复：{}", segment.id);
        ensure!(
            segment.end_ms > segment.start_ms,
            "字幕结束时间必须晚于开始时间"
        );
        ensure!(
            segment.start_ms >= previous_start,
            "字幕片段必须按开始时间排序"
        );
        ensure!(!segment.text.trim().is_empty(), "字幕正文不能为空");
        previous_start = segment.start_ms;
    }
    Ok(())
}
