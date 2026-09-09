use super::settings::AppSettings;
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheCategory {
    pub id: String,
    pub title: String,
    pub path: String,
    pub size_bytes: u64,
    pub file_count: u64,
    pub clearable: bool,
    pub warning: Option<String>,
}

fn directory_size(path: &Path) -> Result<(u64, u64)> {
    if !path.exists() {
        return Ok((0, 0));
    }
    if is_link(&path.symlink_metadata()?) {
        return Ok((0, 0));
    }
    let mut bytes = 0;
    let mut count = 0;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.path().symlink_metadata()?;
        if is_link(&meta) {
            continue;
        }
        if meta.is_dir() {
            let (size, files) = directory_size(&entry.path())?;
            bytes += size;
            count += files;
        } else {
            bytes += meta.len();
            count += 1;
        }
    }
    Ok((bytes, count))
}

fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

fn owned_path(settings: &AppSettings, path: &Path) -> Result<()> {
    let base = settings.data_path();
    let relative = path.strip_prefix(&base).context("不能清理外部目录")?;
    ensure!(!relative.as_os_str().is_empty(), "不能清理资料库根目录");
    let mut current = base;
    for component in relative.components() {
        ensure!(
            matches!(component, std::path::Component::Normal(_)),
            "缓存路径无效"
        );
        current.push(component);
        if current.exists() {
            ensure!(
                !is_link(&current.symlink_metadata()?),
                "缓存路径包含链接或重解析点，请检查目录配置"
            );
        }
    }
    Ok(())
}

fn category_path(settings: &AppSettings, id: &str) -> Result<PathBuf> {
    Ok(match id {
        "audio" | "downloads" | "checkpoints" | "temp" => settings.cache_path(id),
        "logs" => settings.data_path().join("logs"),
        "models" => PathBuf::from(&settings.model_dir),
        _ => anyhow::bail!("未知缓存类别"),
    })
}

pub fn inventory(settings: &AppSettings) -> Result<Vec<CacheCategory>> {
    [
        ("audio", "回听音频"),
        ("downloads", "下载的音轨"),
        ("checkpoints", "转写检查点"),
        ("temp", "临时文件"),
        ("models", "识别模型"),
        ("logs", "运行日志"),
    ]
    .iter()
    .map(|(id, title)| {
        let path = category_path(settings, id)?;
        let (bytes, count) = directory_size(&path)?;
        let clearable = owned_path(settings, &path).is_ok()
            && (*id != "models"
                || (path.join(".course-workbench-owned").is_file()
                    && path.canonicalize().ok()
                        == settings.data_path().join("models").canonicalize().ok()
                    && path.exists()));
        let warning = match *id {
            "checkpoints" => Some("清理后，中断的任务需要从头转写；已保存的文字版本保留。".into()),
            "models" if !clearable => Some("这是外部或共享模型目录，应用仅显示用量。".into()),
            "models" => Some("再次转写前需要重新下载模型。".into()),
            "audio" => Some("本地回听文件会被清理，文字和笔记保留。".into()),
            _ => None,
        };
        Ok(CacheCategory {
            id: (*id).into(),
            title: (*title).into(),
            path: path.to_string_lossy().into_owned(),
            size_bytes: bytes,
            file_count: count,
            clearable,
            warning,
        })
    })
    .collect()
}

pub fn clear(settings: &AppSettings, id: &str) -> Result<()> {
    let category = inventory(settings)?
        .into_iter()
        .find(|c| c.id == id)
        .context("未知缓存类别")?;
    ensure!(category.clearable, "不能清理外部或共享目录");
    let path = category_path(settings, id)?;
    owned_path(settings, &path)?;
    if !path.exists() {
        return Ok(());
    }
    let absolute = path.canonicalize()?;
    let root = settings.data_path().canonicalize()?;
    ensure!(
        absolute.starts_with(&root) && absolute != root,
        "清理路径超出应用资料目录"
    );
    // Refuse reparse-point traversal before every recursion; originals and DB
    // are never categories and cannot be supplied as a raw path from the UI.
    clear_children(&absolute, &root)?;
    Ok(())
}

fn clear_children(directory: &Path, root: &Path) -> Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path
            .file_name()
            .is_some_and(|name| name == ".course-workbench-owned")
        {
            continue;
        }
        let metadata = path.symlink_metadata()?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "缓存含链接，请在文件管理器检查后重试"
        );
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            ensure!(
                metadata.file_attributes() & 0x400 == 0,
                "缓存包含重解析点，已停止清理"
            );
        }
        ensure!(
            path.canonicalize()?.starts_with(root),
            "缓存路径超出资料目录"
        );
        if metadata.is_dir() {
            clear_children(&path, root)?;
            fs::remove_dir(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}
