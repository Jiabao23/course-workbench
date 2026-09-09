use crate::types::{Citation, Segment};
use anyhow::{ensure, Context, Result};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct KnowledgeResponse {
    #[serde(default)]
    content: String,
    #[serde(default)]
    citations: Vec<String>,
    #[serde(default)]
    insufficient_evidence: bool,
}

/// The provider selects IDs; citation text and timestamps always come from the
/// selected local transcript. Provider prose is returned as Markdown data.
pub fn validate_knowledge_response(
    raw: &str,
    segments: &[Segment],
) -> Result<(String, Vec<Citation>)> {
    let payload = strip_fence(raw)?;
    let response: KnowledgeResponse =
        serde_json::from_str(payload).context("知识回复不是约定的 JSON 格式")?;
    let mut source = HashMap::with_capacity(segments.len());
    for segment in segments {
        ensure!(!segment.id.trim().is_empty(), "来源字幕片段 ID 不能为空");
        ensure!(
            source.insert(segment.id.as_str(), segment).is_none(),
            "来源字幕片段 ID 重复：{}",
            segment.id
        );
    }
    let mut seen = HashSet::new();
    let mut citations = Vec::new();
    for id in response.citations {
        let segment = source
            .get(id.as_str())
            .with_context(|| format!("知识回复引用了未知字幕片段：{id}"))?;
        ensure!(
            !segment.text.trim().is_empty() && segment.end_ms > segment.start_ms,
            "引用的字幕片段没有有效证据：{id}"
        );
        if seen.insert(id.clone()) {
            citations.push(Citation {
                segment_id: id,
                start_ms: segment.start_ms,
                end_ms: segment.end_ms,
                text: segment.text.clone(),
            });
        }
    }
    let mut remaining = response.content.as_str();
    while let Some((_, after_marker)) = remaining.split_once("[引用:") {
        let (id, rest) = after_marker
            .split_once(']')
            .context("知识正文的引用标记未闭合")?;
        ensure!(source.contains_key(id), "知识正文引用了未知字幕片段：{id}");
        ensure!(seen.contains(id), "知识正文引用未列入引用数组：{id}");
        remaining = rest;
    }
    // Unknown citations were checked even if the provider also reports that it
    // lacks evidence. Do not retain potentially unsupported provider claims.
    if response.insufficient_evidence {
        return Ok((
            "证据不足：所选字幕无法支持可靠回答，请选择更多相关片段。".into(),
            Vec::new(),
        ));
    }
    ensure!(!response.content.trim().is_empty(), "知识回复正文为空");
    ensure!(!citations.is_empty(), "知识回复缺少可核验的字幕引用");
    Ok((response.content, citations))
}

fn strip_fence(raw: &str) -> Result<&str> {
    let trimmed = raw.trim().trim_start_matches('\u{feff}').trim();
    if !trimmed.starts_with("```") {
        return Ok(trimmed);
    }
    let (opening, rest) = trimmed
        .split_once('\n')
        .context("知识回复的 JSON 代码围栏不完整")?;
    let label = opening.trim().trim_start_matches("```").trim();
    ensure!(
        label.is_empty() || label.eq_ignore_ascii_case("json"),
        "知识回复使用了非 JSON 代码围栏"
    );
    let inner = rest
        .trim_end()
        .strip_suffix("```")
        .context("知识回复的 JSON 代码围栏未闭合")?;
    Ok(inner.trim())
}
