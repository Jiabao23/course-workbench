//! Single-video extraction through yt-dlp; no platform scraping in the UI.
use super::{
    process::{self, ProcessControl},
    settings::{find_program, AppSettings},
    source::{SourcePart, SourcePreview, SourceProvider, SubtitleTrack},
};
use anyhow::{bail, ensure, Context, Result};
use course_core::{subtitles::parse_subtitles, Segment};
use serde_json::Value;
use std::{fs, path::Path, process::Command, time::Duration};
use url::Url;
use uuid::Uuid;

pub fn validate_web_url(source: &str) -> Result<Url> {
    let url = Url::parse(source.trim()).context("请输入完整的 HTTP 或 HTTPS 视频链接")?;
    ensure!(
        matches!(url.scheme(), "http" | "https") && url.host_str().is_some(),
        "视频链接仅支持 HTTP 和 HTTPS"
    );
    ensure!(
        url.username().is_empty() && url.password().is_none(),
        "链接不能包含登录凭据；请在设置中选择网站 Cookie 文件"
    );
    Ok(url)
}

pub fn downloader_command(settings: &AppSettings) -> Result<Command> {
    let mut command = if !settings.yt_dlp_path.trim().is_empty() {
        process::command(&settings.yt_dlp_path)?
    } else {
        ensure!(
            !settings.python_path.trim().is_empty(),
            "请在设置中配置 yt-dlp 可执行文件，或安装了 yt-dlp 的 Python 环境"
        );
        let mut command = process::command(&settings.python_path)?;
        command.args(["-m", "yt_dlp"]);
        command
    };
    command.args([
        "--ignore-config",
        "--no-playlist",
        "--socket-timeout",
        "20",
        "--retries",
        "2",
        "--extractor-retries",
        "1",
        "--no-colors",
    ]);
    if !settings.cookie_file.trim().is_empty() {
        command.arg("--cookies").arg(&settings.cookie_file);
    }
    let node = find_program(&["node.exe", "node"]);
    if !node.is_empty() {
        command.arg("--js-runtimes").arg(format!("node:{node}"));
    }
    if !settings.ffmpeg_path.trim().is_empty() {
        command.arg("--ffmpeg-location").arg(&settings.ffmpeg_path);
    }
    Ok(command)
}

pub fn metadata_command(settings: &AppSettings, source: &str) -> Result<Command> {
    let source = validate_web_url(source)?;
    let mut command = downloader_command(settings)?;
    command.args([
        "--skip-download",
        "--dump-single-json",
        "--flat-playlist",
        "--playlist-end",
        "1",
        "--no-progress",
    ]);
    command.arg("--").arg(source.as_str());
    Ok(command)
}

pub fn subtitle_command(
    settings: &AppSettings,
    source: &str,
    track: &SubtitleTrack,
    directory: &Path,
) -> Result<Command> {
    let source = validate_web_url(source)?;
    ensure!(
        ["vtt", "srt"].contains(&track.format.as_str()),
        "此字幕格式暂不支持直接提取"
    );
    ensure!(
        !track.language.is_empty() && track.language.len() <= 100,
        "字幕语言标记无效"
    );
    let mut command = downloader_command(settings)?;
    command.args(["--skip-download", "--no-simulate", "--no-progress"]);
    command.args(if track.automatic {
        ["--no-write-subs", "--write-auto-subs"]
    } else {
        ["--write-subs", "--no-write-auto-subs"]
    });
    command
        .arg("--sub-langs")
        .arg(format!("^{}$", regex::escape(&track.language)))
        .arg("--sub-format")
        .arg(&track.format)
        .arg("-o")
        .arg(directory.join("subtitle.%(ext)s"))
        .arg("--")
        .arg(source.as_str());
    Ok(command)
}

fn readable_tracks(value: &Value, preferred: &str) -> Vec<SubtitleTrack> {
    let mut tracks = Vec::new();
    let mut languages = std::collections::HashSet::new();
    for (field, automatic) in [("subtitles", false), ("automatic_captions", true)] {
        let Some(groups) = value[field].as_object() else {
            continue;
        };
        for (language, formats) in groups {
            if languages.contains(language) || language.is_empty() || language.len() > 100 {
                continue;
            }
            let Some(formats) = formats.as_array() else {
                continue;
            };
            let candidate = ["vtt", "srt"].into_iter().find_map(|extension| {
                formats.iter().find(|format| {
                    format["ext"].as_str() == Some(extension)
                        && format["url"]
                            .as_str()
                            .is_some_and(|url| validate_web_url(url).is_ok())
                })
            });
            let Some(candidate) = candidate else { continue };
            languages.insert(language.clone());
            let name = candidate["name"].as_str().unwrap_or(language);
            tracks.push((
                automatic,
                SubtitleTrack {
                    language: language.clone(),
                    label: format!(
                        "{name} · {}",
                        if automatic {
                            "自动字幕"
                        } else {
                            "人工字幕"
                        }
                    ),
                    url: candidate["url"].as_str().unwrap_or_default().into(),
                    format: candidate["ext"].as_str().unwrap_or_default().into(),
                    automatic,
                },
            ));
        }
    }
    let preferred = if preferred.is_empty() {
        "zh"
    } else {
        preferred
    };
    let language_rank = |language: &str| {
        if language == preferred || language.starts_with(&format!("{preferred}-")) {
            0
        } else if language == "en" || language.starts_with("en-") {
            1
        } else {
            2
        }
    };
    tracks.sort_by_key(|(automatic, track)| {
        (
            language_rank(&track.language),
            *automatic,
            track.language.clone(),
        )
    });
    tracks.into_iter().map(|(_, track)| track).collect()
}

pub fn parse_metadata(value: &Value, requested: &str, language: &str) -> Result<SourcePreview> {
    validate_web_url(requested)?;
    ensure!(value.is_object(), "网站返回的视频信息无效");
    ensure!(
        !matches!(value["_type"].as_str(), Some("playlist" | "multi_video"))
            && !value["entries"].is_array(),
        "请使用单个视频的链接；当前不导入播放列表或整套合集"
    );
    ensure!(
        !value["is_live"].as_bool().unwrap_or(false)
            && !matches!(
                value["live_status"].as_str(),
                Some("is_live" | "is_upcoming")
            ),
        "请在直播或预约视频结束后，使用可回放的视频链接"
    );
    ensure!(
        !value["has_drm"].as_bool().unwrap_or(false),
        "该来源有内容保护，当前无法导入"
    );
    let title = value["title"]
        .as_str()
        .filter(|title| !title.trim().is_empty())
        .context("网站没有返回单个视频的标题")?;
    let source = value["webpage_url"]
        .as_str()
        .filter(|source| validate_web_url(source).is_ok())
        .unwrap_or(requested);
    let duration = value["duration"].as_f64().unwrap_or(0.0);
    ensure!(duration.is_finite() && duration >= 0.0, "视频时长无效");
    let subtitles = readable_tracks(value, language);
    let subtitle_status = if !subtitles.is_empty() {
        "available"
    } else if matches!(
        value["availability"].as_str(),
        Some("needs_auth" | "premium_only" | "subscriber_only" | "private")
    ) {
        "loginRequired"
    } else {
        "absent"
    };
    let audio_only = value["vcodec"].as_str() == Some("none")
        || value["formats"].as_array().is_some_and(|formats| {
            formats.iter().any(|format| {
                format["vcodec"].as_str() == Some("none")
                    && format["acodec"].as_str() != Some("none")
            })
        });
    let mut warnings = Vec::new();
    if !audio_only {
        warnings
            .push("该来源未提供独立音轨；需要转写或本地回听时，将下载原媒体再提取音频。".into());
    }
    if duration == 0.0 {
        warnings.push("网站未提供时长，将在获取音频后检测。".into());
    }
    if subtitles.is_empty()
        && ["subtitles", "automatic_captions"].iter().any(|field| {
            value[*field]
                .as_object()
                .is_some_and(|tracks| !tracks.is_empty())
        })
    {
        warnings.push("网站返回了字幕，但没有可直接读取的 SRT/VTT 格式，可选择音频转写。".into());
    }
    Ok(SourcePreview {
        source: source.into(),
        title: title.into(),
        source_kind: "webMedia".into(),
        bvid: None,
        parts: vec![SourcePart {
            page: 1,
            cid: None,
            title: title.into(),
            duration_ms: (duration * 1000.0).round() as u64,
            subtitle_status: subtitle_status.into(),
            subtitles,
        }],
        warnings,
    })
}

pub fn extractor_error(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let hint = if lower.contains("unsupported url") || lower.contains("no suitable extractor") {
        "当前 yt-dlp 版本不支持此链接；请更新下载工具，或从本地文件导入。"
    } else if ["sign in", "login", "authentication", "cookies", "captcha"]
        .iter()
        .any(|word| lower.contains(word))
    {
        "网站要求登录或验证；请在设置中配置该网站的 Cookie 文件后重试。"
    } else if lower.contains("429") || lower.contains("too many requests") {
        "网站暂时限制了请求，请稍后重试。"
    } else if lower.contains("drm") {
        "该来源有内容保护，当前无法导入。"
    } else {
        "网站解析失败，请检查链接、网络和 yt-dlp 版本；也可以从本地文件导入。"
    };
    format!("{hint}\n{error}")
}

pub struct WebProvider<'a> {
    settings: &'a AppSettings,
    control: &'a ProcessControl,
}
impl<'a> WebProvider<'a> {
    pub fn new(settings: &'a AppSettings, control: &'a ProcessControl) -> Self {
        Self { settings, control }
    }
    fn capture(&self, command: Command, operation: &str) -> Result<String> {
        let log = self
            .settings
            .data_path()
            .join("logs")
            .join(format!("web-{operation}-{}.log", Uuid::new_v4()));
        process::capture(command, self.control, &log, Duration::from_secs(120))
            .map_err(|error| anyhow::anyhow!(extractor_error(&format!("{error:#}"))))
    }
}
impl SourceProvider for WebProvider<'_> {
    fn preview(&self, source: &str) -> Result<SourcePreview> {
        let raw = self.capture(metadata_command(self.settings, source)?, "preview")?;
        let value: Value =
            serde_json::from_str(&raw).context("yt-dlp 未返回有效的视频 JSON 信息")?;
        parse_metadata(&value, source, &self.settings.language)
    }
    fn inspect_part(&self, source: &str, part: &mut SourcePart) -> Result<()> {
        ensure!(part.page == 1, "通用视频链接只支持单个视频");
        *part = self
            .preview(source)?
            .parts
            .into_iter()
            .next()
            .context("链接没有可处理的视频")?;
        Ok(())
    }
    fn subtitles(&self, source: &str, track: &SubtitleTrack) -> Result<Vec<Segment>> {
        let cache = self.settings.cache_path("temp");
        fs::create_dir_all(&cache)?;
        let directory = tempfile::Builder::new()
            .prefix("captions-")
            .tempdir_in(cache)?;
        self.capture(
            subtitle_command(self.settings, source, track, directory.path())?,
            "subtitles",
        )?;
        self.control.check()?;
        for entry in fs::read_dir(directory.path())? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_file()
                && path.extension().and_then(|ext| ext.to_str()) == Some(track.format.as_str())
            {
                ensure!(entry.metadata()?.len() < 32 * 1024 * 1024, "字幕文件过大");
                return parse_subtitles(&fs::read_to_string(path)?, &track.format);
            }
        }
        bail!("网站未返回所选字幕文件，请重新读取链接或选择音频转写")
    }
}
