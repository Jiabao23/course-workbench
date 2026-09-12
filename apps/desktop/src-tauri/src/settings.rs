use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
};
use url::Url;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub data_dir: String,
    pub model_dir: String,
    pub python_path: String,
    pub ffmpeg_path: String,
    pub ffprobe_path: String,
    pub yt_dlp_path: String,
    pub preset: String,
    pub model: String,
    pub device: String,
    pub threads: u32,
    pub gpu_concurrency: u32,
    pub language: String,
    pub prompt: String,
    pub llm_base_url: String,
    pub llm_model: String,
    pub llm_context_chars: usize,
    pub cookie_file: String,
    pub setup_complete: bool,
    pub obsidian_vault: String,
    pub theme: String,
}

pub fn find_program(names: &[&str]) -> String {
    for name in names {
        let path = Path::new(name);
        if path.is_absolute() && path.is_file() {
            return path.to_string_lossy().into_owned();
        }
        if let Some(paths) = env::var_os("PATH") {
            for directory in env::split_paths(&paths) {
                let candidate = directory.join(name);
                if candidate.is_file() {
                    return candidate.to_string_lossy().into_owned();
                }
            }
        }
    }
    String::new()
}

impl Default for AppSettings {
    fn default() -> Self {
        let data = if cfg!(debug_assertions) {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.local-data/library")
        } else if Path::new("D:\\").exists() {
            PathBuf::from("D:\\CourseWorkbenchData")
        } else {
            PathBuf::from(env::var_os("USERPROFILE").unwrap_or_default())
                .join("Documents/CourseWorkbench")
        };
        // Known working environment is a discovery candidate, never a requirement.
        let shared_model = Path::new("D:/bili2text/.cache/whisper");
        let model_dir = if shared_model.join("small.pt").is_file() {
            shared_model.to_owned()
        } else {
            data.join("models")
        };
        let screenpipe =
            PathBuf::from(env::var_os("USERPROFILE").unwrap_or_default()).join("screenpipe/bin");
        let ffmpeg_candidate = screenpipe.join("ffmpeg.exe");
        let ffprobe_candidate = screenpipe.join("ffprobe.exe");
        Self {
            data_dir: data.to_string_lossy().into_owned(),
            model_dir: model_dir.to_string_lossy().into_owned(),
            python_path: find_program(&[
                "D:/bili2text/.venv/Scripts/python.exe",
                "python.exe",
                "python3",
                "python",
            ]),
            ffmpeg_path: find_program(&[
                &ffmpeg_candidate.to_string_lossy(),
                "ffmpeg.exe",
                "ffmpeg",
            ]),
            ffprobe_path: find_program(&[
                &ffprobe_candidate.to_string_lossy(),
                "ffprobe.exe",
                "ffprobe",
            ]),
            yt_dlp_path: find_program(&["yt-dlp.exe", "yt-dlp"]),
            preset: "balanced".into(),
            model: "small".into(),
            device: "auto".into(),
            threads: 4,
            gpu_concurrency: 1,
            language: "zh".into(),
            prompt: String::new(),
            llm_base_url: "https://api.openai.com/v1".into(),
            llm_model: String::new(),
            llm_context_chars: 24000,
            cookie_file: String::new(),
            setup_complete: false,
            obsidian_vault: String::new(),
            theme: "forest".into(),
        }
    }
}

impl AppSettings {
    pub fn data_path(&self) -> PathBuf {
        PathBuf::from(&self.data_dir)
    }
    pub fn cache_path(&self, category: &str) -> PathBuf {
        self.data_path().join("cache").join(category)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            ["forest", "paper", "night"].contains(&self.theme.as_str()),
            "未知主题，请选择森林浅色、暖纸米色或深海夜色"
        );
        ensure!(
            self.obsidian_vault.is_empty()
                || (Path::new(&self.obsidian_vault).is_absolute()
                    && Path::new(&self.obsidian_vault).parent().is_some()),
            "Obsidian 知识库必须是绝对文件夹路径"
        );
        ensure!(
            Path::new(&self.data_dir).is_absolute(),
            "资料目录必须是绝对路径"
        );
        ensure!(
            Path::new(&self.data_dir).parent().is_some(),
            "请选择磁盘中的一个文件夹，不能直接使用磁盘根目录"
        );
        ensure!(
            Path::new(&self.model_dir).is_absolute(),
            "模型目录必须是绝对路径"
        );
        ensure!(
            ["eco", "balanced", "quality", "custom"].contains(&self.preset.as_str()),
            "未知资源档位"
        );
        ensure!(
            ["auto", "cpu", "cuda"].contains(&self.device.as_str()),
            "首版支持自动、CPU 与 NVIDIA CUDA"
        );
        ensure!((1..=256).contains(&self.threads), "线程数超出范围");
        ensure!(
            self.gpu_concurrency == 1,
            "首版 GPU 并发固定为 1；更高并发尚未通过资源验证"
        );
        ensure!(
            [
                "tiny",
                "tiny.en",
                "base",
                "base.en",
                "small",
                "small.en",
                "medium",
                "medium.en",
                "large",
                "large-v1",
                "large-v2",
                "large-v3",
                "turbo",
                "large-v3-turbo"
            ]
            .contains(&self.model.as_str()),
            "未知 Whisper 模型"
        );
        ensure!(
            self.prompt.chars().count() <= 4000,
            "术语提示最多 4000 字符"
        );
        ensure!(
            (1000..=200000).contains(&self.llm_context_chars),
            "单次发送文字上限应为 1000 到 200000 字符"
        );
        if !self.llm_base_url.is_empty() {
            super::knowledge::validate_endpoint(&self.llm_base_url)?;
        }
        Ok(())
    }
    pub fn prepare_directories(&self) -> Result<()> {
        for directory in [
            self.data_path(),
            self.data_path().join("jobs"),
            self.data_path().join("logs"),
            self.data_path().join("benchmarks"),
            self.data_path().join("exports"),
            self.cache_path("audio"),
            self.cache_path("downloads"),
            self.cache_path("checkpoints"),
            self.cache_path("temp"),
        ] {
            fs::create_dir_all(&directory)
                .with_context(|| format!("无法创建目录 {}", directory.display()))?;
        }
        Ok(())
    }
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = env::var_os("COURSE_WORKBENCH_CONFIG") {
        return PathBuf::from(path);
    }
    if cfg!(debug_assertions) {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../.local-data/settings.json")
    } else {
        PathBuf::from(env::var_os("LOCALAPPDATA").unwrap_or_else(|| ".".into()))
            .join("CourseWorkbench/settings.json")
    }
}

pub fn load(path: &Path) -> Result<AppSettings> {
    let settings = if path.exists() {
        serde_json::from_slice(&fs::read(path)?)
            .context("设置文件损坏，请从诊断目录检查 settings.json")?
    } else {
        AppSettings::default()
    };
    settings.validate()?;
    settings.prepare_directories()?;
    Ok(settings)
}

pub fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = fs::File::create(&temp)?;
    file.write_all(content)?;
    file.sync_all()?;
    drop(file);
    // MoveFileEx replaces atomically on Windows; rename does so on Unix.
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let from: Vec<u16> = temp.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        if unsafe {
            MoveFileExW(
                from.as_ptr(),
                to.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        } == 0
        {
            let error = std::io::Error::last_os_error();
            let _ = fs::remove_file(&temp);
            return Err(error.into());
        }
    }
    #[cfg(not(windows))]
    fs::rename(&temp, path)?;
    Ok(())
}

pub fn save(path: &Path, settings: &AppSettings) -> Result<()> {
    settings.validate()?;
    atomic_write(path, &serde_json::to_vec_pretty(settings)?)
}

pub fn safe_web_url(target: &str) -> Result<Url> {
    let url = Url::parse(target)?;
    ensure!(
        ["https", "http"].contains(&url.scheme())
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none(),
        "只支持 http/https 网页链接"
    );
    Ok(url)
}
