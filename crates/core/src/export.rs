use crate::{subtitles::validate_segments, types::Transcript};
use anyhow::{bail, Result};
use std::fmt::Write;

pub fn export_transcript(
    transcript: &Transcript,
    title: &str,
    source_url: Option<&str>,
    format: &str,
) -> Result<String> {
    validate_segments(&transcript.segments)?;
    let format = format.trim().trim_start_matches('.').to_ascii_lowercase();
    let mut output = String::new();
    match format.as_str() {
        "srt" | "vtt" => {
            let separator = if format == "srt" { ',' } else { '.' };
            if format == "vtt" {
                output.push_str("WEBVTT\n\n");
            }
            for (index, segment) in transcript.segments.iter().enumerate() {
                let cue = segment
                    .text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .lines()
                    .filter(|line| !line.trim().is_empty())
                    .collect::<Vec<_>>()
                    .join("\n");
                let cue = if format == "vtt" {
                    cue.replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;")
                } else {
                    cue
                };
                if format == "srt" {
                    writeln!(output, "{}", index + 1)?;
                }
                writeln!(
                    output,
                    "{} --> {}\n{}\n",
                    timestamp(segment.start_ms, separator),
                    timestamp(segment.end_ms, separator),
                    // A blank line terminates an SRT/VTT cue. Preserve the
                    // original in storage/TXT/MD but normalize cue payloads.
                    cue
                )?;
            }
        }
        "txt" | "md" => {
            if format == "md" {
                writeln!(output, "# {}", title.replace(['\r', '\n'], " "))?;
            } else {
                writeln!(output, "{title}")?;
            }
            if let Some(source) = source_url.filter(|source| !source.trim().is_empty()) {
                writeln!(output, "\n来源：{source}")?;
            }
            writeln!(output, "\n转写版本：{}\n", transcript.version)?;
            for segment in &transcript.segments {
                let time = timestamp(segment.start_ms, '.');
                if format == "md" {
                    let anchor = segment
                        .id
                        .replace('&', "&amp;")
                        .replace('"', "&quot;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    writeln!(
                        output,
                        "<a id=\"segment-{anchor}\"></a>\n\n**[{time}]**\n\n{}\n",
                        segment.text
                    )?;
                } else {
                    writeln!(output, "[{time}]\n{}\n", segment.text)?;
                }
            }
        }
        _ => bail!("不支持的导出格式：{format}"),
    }
    Ok(output)
}

fn timestamp(milliseconds: u64, separator: char) -> String {
    let hours = milliseconds / 3_600_000;
    let minutes = milliseconds / 60_000 % 60;
    let seconds = milliseconds / 1000 % 60;
    let fraction = milliseconds % 1000;
    format!("{hours:02}:{minutes:02}:{seconds:02}{separator}{fraction:03}")
}
