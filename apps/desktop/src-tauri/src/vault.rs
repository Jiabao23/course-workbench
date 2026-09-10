//! Plugin-free Obsidian bridge. Immutable snapshots and append-only indexes.
use crate::integrity::IntegrityReport;
use anyhow::{ensure, Context, Result};
use course_core::{Asset, Note, Transcript};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use url::Url;

const MARKER: &str = "<!-- course-workbench-index-v1 -->";
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncResult {
    pub snapshot_path: String,
    pub index_path: String,
    pub personal_path: String,
    pub snapshot_link: String,
    pub open_uri: String,
    pub created: bool,
}
fn no_links(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(meta) => {
                let mut link = meta.file_type().is_symlink();
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    link |= meta.file_attributes() & 0x400 != 0;
                }
                ensure!(
                    !link,
                    "知识库路径不能经过符号链接或目录联接：{}",
                    ancestor.display()
                );
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
fn directory(path: &Path) -> Result<()> {
    no_links(path)?;
    fs::create_dir_all(path)?;
    no_links(path)?;
    ensure!(path.is_dir(), "知识库路径不是目录");
    Ok(())
}
fn root_path(path: &Path) -> Result<PathBuf> {
    ensure!(
        path.is_absolute() && path.parent().is_some(),
        "请选择磁盘中的独立知识库文件夹，不能使用相对路径或磁盘根目录"
    );
    ensure!(
        !path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir)),
        "知识库路径不能包含上级目录"
    );
    no_links(path)?;
    Ok(path.to_path_buf())
}
pub fn initialize(path: &Path) -> Result<String> {
    let root = root_path(path)?;
    directory(&root)?;
    directory(&root.join(".obsidian"))?;
    directory(&root.join("CourseWorkbench"))?;
    Ok(dunce::canonicalize(root)?.to_string_lossy().into_owned())
}
fn valid_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 80
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "资料标识不能用于文件路径"
    );
    Ok(())
}
pub fn block_id(id: &str) -> String {
    format!("s-{}", hex::encode(Sha256::digest(id.as_bytes())))
}
fn plain(text: &str) -> String {
    text.replace(['\r', '\n', '[', ']', '|', '<', '>'], " ")
}
fn quote(text: &str) -> String {
    text.lines()
        .map(|l| format!("> {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}
fn immutable(path: &Path, text: &str) -> Result<bool> {
    no_links(path)?;
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("文件缺少目录")?)?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    match tmp.persist_noclobber(path) {
        Ok(_) => Ok(true),
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                fs::metadata(path)?.len() == text.len() as u64
                    && fs::read(path)? == text.as_bytes(),
                "同步冲突：{} 已存在且内容被修改，请保留个人修改并移走该文件后重试。",
                path.display()
            );
            Ok(false)
        }
        Err(e) => Err(e.error.into()),
    }
}
fn append_index(path: &Path, heading: &str, link: &str) -> Result<()> {
    no_links(path)?;
    if !path.exists() {
        immutable(
            path,
            &format!("{MARKER}\n# {heading}\n\n可在索引中补充自己的分类与链接。\n"),
        )?;
    }
    let mut file = OpenOptions::new().read(true).append(true).open(path)?;
    ensure!(
        file.metadata()?.len() < 8 * 1024 * 1024,
        "索引文件过大，请整理后重试"
    );
    let mut text = String::new();
    file.read_to_string(&mut text)?;
    ensure!(
        text.starts_with(MARKER),
        "索引文件冲突：已有同名文件不属于课程工作台，已保留原文。"
    );
    if !text.lines().any(|line| line == format!("- {link}")) {
        writeln!(file, "\n- {link}")?;
        file.sync_all()?;
    }
    Ok(())
}
pub fn index_path(root: &Path, asset_id: &str) -> Result<PathBuf> {
    valid_id(asset_id)?;
    let root = root_path(root)?;
    let path = root
        .join("CourseWorkbench")
        .join(asset_id)
        .join("课程索引.md");
    no_links(&path)?;
    ensure!(path.is_file(), "此课程尚未同步到知识库，请先同步");
    Ok(dunce::canonicalize(path)?)
}
pub fn open_uri(path: &Path) -> Result<String> {
    let mut url = Url::parse("obsidian://open")?;
    url.query_pairs_mut()
        .append_pair("path", &path.to_string_lossy());
    Ok(url.to_string())
}
pub fn sync(
    root: &Path,
    asset: &Asset,
    t: &Transcript,
    notes: &[Note],
    report: &IntegrityReport,
) -> Result<SyncResult> {
    let root = root_path(root)?;
    ensure!(
        root.join(".obsidian").is_dir(),
        "请先在设置中初始化或连接 Obsidian 知识库"
    );
    no_links(&root.join(".obsidian"))?;
    valid_id(&asset.id)?;
    valid_id(&t.id)?;
    ensure!(
        t.asset_id == asset.id && report.transcript_id == t.id,
        "同步版本与课程不匹配"
    );
    for n in notes {
        ensure!(
            n.asset_id == asset.id && n.transcript_id == t.id,
            "笔记不属于此文字版本"
        );
        for c in &n.citations {
            ensure!(
                t.segments.iter().any(|s| s.id == c.segment_id),
                "引用片段不存在，停止同步"
            );
        }
    }
    let managed = root.join("CourseWorkbench");
    directory(&managed)?;
    let lock_path = managed.join(".sync.lock");
    no_links(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(lock_path)?;
    lock.try_lock()
        .context("另一个同步正在写入知识库，请稍后重试")?;
    let course = managed.join(&asset.id);
    directory(&course)?;
    let mut body=format!("---\ncourse_workbench: true\nasset_id: {}\ntranscript_id: {}\nversion: {}\ntags: [course-workbench]\n---\n\n# {}\n\n来源：{}\n\n[[CourseWorkbench/{}/个人笔记|我的补充]] · [[CourseWorkbench/{}/课程索引|历史快照]]\n\n## 完整性核对\n\n{}\n\n时间轴覆盖 {} ms / {}；{} 段。覆盖比例不是准确率。\n",asset.id,t.id,t.version,plain(&asset.title),plain(&asset.source),asset.id,asset.id,report.limitations,report.covered_ms,report.duration_ms.map_or("来源时长未知".into(),|d|format!("{d} ms")),report.segment_count);
    if report.issues.is_empty() {
        body.push_str("\n未发现明显时间轴异常。\n");
    }
    for i in &report.issues {
        body.push_str(&format!(
            "\n- {}–{} ms：{}\n",
            i.start_ms, i.end_ms, i.message
        ));
    }
    if let Some(review) = &report.review {
        body.push_str(&format!(
            "\n人工核对（{}）：\n\n{}\n",
            review.reviewed_at,
            quote(&review.note)
        ));
    }
    body.push_str("\n## 原文\n");
    for s in &t.segments {
        body.push_str(&format!(
            "\n### {} ms\n\n{}\n\n^{}\n",
            s.start_ms,
            quote(&s.text),
            block_id(&s.id)
        ));
    }
    for n in notes {
        let mut content = n.content.clone();
        for c in &n.citations {
            content = content.replace(
                &format!("[引用:{}]", c.segment_id),
                &format!("[[#^{}|{} ms 原文]]", block_id(&c.segment_id), c.start_ms),
            );
        }
        body.push_str(&format!("\n## {}\n\n{}\n", plain(&n.title), content));
        for c in &n.citations {
            body.push_str(&format!(
                "\n- [[#^{}|{} ms 原文依据]]\n",
                block_id(&c.segment_id),
                c.start_ms
            ));
        }
    }
    let digest = hex::encode(Sha256::digest(body.as_bytes()));
    let name = format!("v{}-{}-{digest}", t.version, t.id);
    let snapshot = course.join(format!("{name}.md"));
    let created = immutable(&snapshot, &body)?;
    let personal = course.join("个人笔记.md");
    no_links(&personal)?;
    if !personal.exists() {
        immutable(&personal,&format!("# {}：我的笔记\n\n此文件供你自由编辑，同步不会覆盖。\n\n[[CourseWorkbench/{}/课程索引|课程快照与出处]]\n",plain(&asset.title),asset.id))?;
    }
    let snapshot_link = format!(
        "[[CourseWorkbench/{}/{name}|版本 {} · 快照 {}]]",
        asset.id,
        t.version,
        &digest[..8]
    );
    let index = course.join("课程索引.md");
    append_index(&index, &plain(&asset.title), &snapshot_link)?;
    append_index(
        &managed.join("课程索引.md"),
        "课程知识索引",
        &format!(
            "[[CourseWorkbench/{}/课程索引|{}]]",
            asset.id,
            plain(&asset.title)
        ),
    )?;
    let index = dunce::canonicalize(index)?;
    Ok(SyncResult {
        snapshot_path: dunce::canonicalize(snapshot)?
            .to_string_lossy()
            .into_owned(),
        index_path: index.to_string_lossy().into_owned(),
        personal_path: dunce::canonicalize(personal)?
            .to_string_lossy()
            .into_owned(),
        snapshot_link,
        open_uri: open_uri(&index)?,
        created,
    })
}
