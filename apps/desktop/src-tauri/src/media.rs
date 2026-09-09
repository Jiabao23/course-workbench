use super::{
    process::{self, ProcessControl},
    settings::AppSettings,
};
use anyhow::{ensure, Context, Result};
use course_core::Asset;
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub fn duration_ms(settings: &AppSettings, path: &Path, control: &ProcessControl) -> Result<u64> {
    let mut command = process::command(&settings.ffprobe_path)?;
    command
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path);
    let value = process::capture(
        command,
        control,
        &settings.data_path().join("logs/ffprobe.log"),
        Duration::from_secs(30),
    )?;
    let seconds: f64 = value
        .trim()
        .parse()
        .context("无法读取音视频时长，请检查文件是否损坏")?;
    ensure!(seconds.is_finite() && seconds > 0.0, "音视频没有有效时长");
    Ok((seconds * 1000.0).round() as u64)
}

pub fn convert(
    settings: &AppSettings,
    input: &Path,
    output: &Path,
    control: &ProcessControl,
    limit_seconds: Option<u32>,
    mut progress: impl FnMut(f64) -> Result<()>,
) -> Result<()> {
    let duration = duration_ms(settings, input, control)?;
    let expected = limit_seconds
        .map(|limit| duration.min(u64::from(limit) * 1000))
        .unwrap_or(duration);
    let partial = output.with_extension("partial.wav");
    if let Some(parent) = partial.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut command = process::command(&settings.ffmpeg_path)?;
    command
        .args(["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"])
        .arg(input)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le"]);
    if let Some(seconds) = limit_seconds {
        command.args(["-t", &seconds.to_string()]);
    }
    command
        .args(["-progress", "pipe:1", "-nostats"])
        .arg(&partial);
    process::run_lines(
        command,
        None,
        control,
        &settings.data_path().join("logs/ffmpeg.log"),
        Duration::from_secs(24 * 3600),
        |line| {
            if let Some(micros) = line
                .strip_prefix("out_time_us=")
                .and_then(|v| v.parse::<u64>().ok())
            {
                progress(
                    (micros as f64 / 1000.0 / expected.max(1) as f64 * 100.0).clamp(0.0, 100.0),
                )?;
            }
            Ok(())
        },
    )?;
    control.check()?;
    let actual = duration_ms(settings, &partial, control)?;
    ensure!(actual.abs_diff(expected) < 1500, "音频转换不完整，请重试");
    fs::rename(&partial, output).context("无法保存转换后的音轨")?;
    Ok(())
}

fn fingerprint(path: &Path) -> Result<String> {
    let mut digest = Sha256::new();
    let mut input = fs::File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let length = input.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(hex::encode(digest.finalize())[..20].to_owned())
}

pub fn download(
    settings: &AppSettings,
    asset: &Asset,
    control: &ProcessControl,
    mut progress: impl FnMut(f64) -> Result<()>,
) -> Result<PathBuf> {
    let directory = settings.cache_path("downloads");
    let prefix = format!("{}.", asset.id);
    for entry in fs::read_dir(&directory)? {
        let path = entry?.path();
        let extension = path.extension().and_then(|s| s.to_str()).unwrap_or("");
        if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(&prefix))
            && ["m4a", "aac", "mp3", "webm", "opus", "ogg", "flac"].contains(&extension)
        {
            return Ok(path);
        }
    }
    let mut command = if settings.yt_dlp_path.is_empty() {
        let mut command = process::command(&settings.python_path)?;
        command.args(["-m", "yt_dlp"]);
        command
    } else {
        process::command(&settings.yt_dlp_path)?
    };
    let output = directory.join(format!("{}.%(ext)s", asset.id));
    let source = format!(
        "https://www.bilibili.com/video/{}?p={}",
        asset.bvid.as_deref().context("缺少 BV 号")?,
        asset.page.unwrap_or(1)
    );
    command
        .args([
            "--ignore-config",
            "--no-playlist",
            "--no-overwrites",
            "--newline",
            "--socket-timeout",
            "20",
            "--retries",
            "3",
            "--fragment-retries",
            "3",
            "-f",
            "bestaudio",
            "--progress",
            "--progress-template",
            "download:CW_PROGRESS:%(progress._percent_str)s",
            "--print",
            "after_move:CW_PATH:%(filepath)s",
            "-o",
        ])
        .arg(&output);
    if !settings.ffmpeg_path.is_empty() {
        command.arg("--ffmpeg-location").arg(&settings.ffmpeg_path);
    }
    if !settings.cookie_file.is_empty() {
        command.arg("--cookies").arg(&settings.cookie_file);
    }
    command.arg("--").arg(source);
    let mut final_path = None;
    process::run_lines(
        command,
        None,
        control,
        &settings
            .data_path()
            .join("logs")
            .join(format!("download-{}.log", asset.id)),
        Duration::from_secs(24 * 3600),
        |line| {
            if let Some(value) = line.strip_prefix("CW_PROGRESS:") {
                if let Ok(number) = value.trim().trim_end_matches('%').parse::<f64>() {
                    progress(number.clamp(0.0, 100.0))?;
                }
            }
            if let Some(path) = line.strip_prefix("CW_PATH:") {
                final_path = Some(PathBuf::from(path.trim()));
            }
            Ok(())
        },
    )?;
    let path = final_path
        .context("下载工具未返回音轨文件路径，请查看下载日志")?
        .canonicalize()?;
    ensure!(
        path.starts_with(directory.canonicalize()?) && path.is_file(),
        "下载路径不在应用缓存目录内"
    );
    Ok(path)
}

pub fn ensure_audio(
    settings: &AppSettings,
    asset: &Asset,
    control: &ProcessControl,
    mut progress: impl FnMut(&str, f64) -> Result<()>,
) -> Result<PathBuf> {
    if asset.source_kind == "bilibili" {
        if let Some(audio) = asset.audio_path.as_ref().filter(|p| Path::new(p).is_file()) {
            return Ok(PathBuf::from(audio));
        }
    }
    let source = if asset.source_kind == "bilibili" {
        progress("download", 0.0)?;
        download(settings, asset, control, |p| progress("download", p))?
    } else {
        PathBuf::from(&asset.source)
    };
    ensure!(source.is_file(), "原始音视频已移动或删除，请重新选择文件");
    let output =
        settings
            .cache_path("audio")
            .join(format!("{}-{}.wav", asset.id, fingerprint(&source)?));
    if !output.is_file() {
        progress("convert", 0.0)?;
        convert(settings, &source, &output, control, None, |p| {
            progress("convert", p)
        })?;
    }
    Ok(output)
}
