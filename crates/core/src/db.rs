use crate::{
    subtitles::validate_segments,
    types::{Asset, Citation, Job, Note, SearchHit, Segment, Transcript},
};
use anyhow::{ensure, Context, Result};
use chrono::{SecondsFormat, Utc};
use jieba_rs::Jieba;
use rusqlite::{params, types::Type, Connection, OptionalExtension, Row, TransactionBehavior};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::OnceLock,
    time::Duration,
};
use uuid::Uuid;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS assets (
    id TEXT PRIMARY KEY NOT NULL,
    title TEXT NOT NULL,
    source_kind TEXT NOT NULL,
    source TEXT NOT NULL,
    bvid TEXT,
    page INTEGER,
    duration_ms INTEGER NOT NULL CHECK (duration_ms >= 0),
    audio_path TEXT,
    active_version_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY (id, active_version_id) REFERENCES transcripts(asset_id, id)
        DEFERRABLE INITIALLY DEFERRED
);
CREATE TABLE IF NOT EXISTS transcripts (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    version INTEGER NOT NULL CHECK (version > 0),
    source_kind TEXT NOT NULL,
    model TEXT,
    language TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (asset_id, version),
    UNIQUE (asset_id, id)
);
CREATE TABLE IF NOT EXISTS segments (
    transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
    id TEXT NOT NULL,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms > start_ms),
    text TEXT NOT NULL,
    PRIMARY KEY (transcript_id, id),
    UNIQUE (transcript_id, ordinal)
);
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    title TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('queued','running','paused','completed','failed','cancelled')),
    stage TEXT NOT NULL,
    progress REAL NOT NULL CHECK (progress >= 0 AND progress <= 100),
    error TEXT,
    model TEXT NOT NULL,
    device TEXT NOT NULL,
    mode TEXT NOT NULL CHECK (mode IN ('auto','subtitlesOnly','transcribe')),
    preset TEXT NOT NULL CHECK (preset IN ('eco','balanced','quality','custom')),
    chunk_done INTEGER NOT NULL CHECK (chunk_done >= 0),
    chunk_total INTEGER NOT NULL CHECK (chunk_total >= chunk_done),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS notes (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL REFERENCES assets(id),
    transcript_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('summary','answer','manual')),
    title TEXT NOT NULL,
    content TEXT NOT NULL,
    citations_json TEXT NOT NULL,
    question TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (asset_id, transcript_id) REFERENCES transcripts(asset_id, id)
);
CREATE INDEX IF NOT EXISTS notes_by_asset ON notes(asset_id, created_at);
CREATE INDEX IF NOT EXISTS jobs_by_status ON jobs(status, created_at);
CREATE TABLE IF NOT EXISTS job_transcripts (
    job_id TEXT PRIMARY KEY NOT NULL REFERENCES jobs(id),
    transcript_id TEXT NOT NULL REFERENCES transcripts(id)
);
CREATE VIRTUAL TABLE IF NOT EXISTS segment_fts USING fts5(
    asset_id UNINDEXED,
    transcript_id UNINDEXED,
    segment_id UNINDEXED,
    start_ms UNINDEXED,
    end_ms UNINDEXED,
    text UNINDEXED,
    tokens,
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS integrity_reviews (
    transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    note TEXT NOT NULL,
    reviewed_at TEXT NOT NULL,
    PRIMARY KEY(transcript_id, fingerprint)
);
CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    name_key TEXT NOT NULL,
    parent_id TEXT REFERENCES collections(id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX IF NOT EXISTS collection_sibling_names ON collections(IFNULL(parent_id,''),name_key);
CREATE TABLE IF NOT EXISTS asset_organization (
    asset_id TEXT PRIMARY KEY NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    collection_id TEXT REFERENCES collections(id) ON DELETE RESTRICT,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK(favorite IN (0,1))
);
CREATE TABLE IF NOT EXISTS issue_reviews (
    transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
    fingerprint TEXT NOT NULL,
    issue_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK(status IN ('pending','confirmed','revised')),
    note TEXT NOT NULL,
    reviewed_at TEXT NOT NULL,
    PRIMARY KEY(transcript_id,fingerprint,issue_id)
);
CREATE TABLE IF NOT EXISTS quality_evidence (
    transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('diagnostics','speech','provenance')),
    payload TEXT NOT NULL,
    PRIMARY KEY(transcript_id,kind)
);
CREATE TABLE IF NOT EXISTS quality_evidence_history (
    transcript_id TEXT NOT NULL REFERENCES transcripts(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK(kind IN ('diagnostics','speech','provenance')),
    payload TEXT NOT NULL,
    archived_at TEXT NOT NULL,
    PRIMARY KEY(transcript_id,kind,payload)
);
CREATE TABLE IF NOT EXISTS recheck_candidates (
    id TEXT PRIMARY KEY NOT NULL,
    asset_id TEXT NOT NULL,
    transcript_id TEXT NOT NULL,
    payload TEXT NOT NULL,
    adopted_transcript_id TEXT REFERENCES transcripts(id),
    FOREIGN KEY(asset_id,transcript_id) REFERENCES transcripts(asset_id,id)
);
CREATE INDEX IF NOT EXISTS candidates_by_base ON recheck_candidates(asset_id,transcript_id);
PRAGMA user_version = 5;
"#;

const ASSET_COLUMNS: &str = "id, title, source_kind, source, bvid, page, duration_ms, audio_path, active_version_id, created_at, updated_at";
const JOB_COLUMNS: &str = "id, asset_id, title, status, stage, progress, error, model, device, mode, preset, chunk_done, chunk_total, created_at, updated_at";

#[derive(Clone, Debug)]
pub struct Db {
    path: PathBuf,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        ensure!(
            !path.as_os_str().is_empty() && path != Path::new(":memory:"),
            "数据库需要持久化文件路径"
        );
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()?.join(path)
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).context("无法创建数据库目录")?;
        }
        let db = Self { path };
        let mut connection = db.connection()?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        ensure!(version <= 5, "数据库版本较新，请升级应用后打开");
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction
            .execute_batch(SCHEMA)
            .context("无法初始化数据库结构或 FTS5 索引")?;
        transaction.commit()?;
        Ok(db)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn transcript_job(&self, transcript_id: &str) -> Result<Option<Job>> {
        let id: Option<String> = self
            .connection()?
            .query_row(
                "SELECT job_id FROM job_transcripts WHERE transcript_id=?1",
                [transcript_id],
                |row| row.get(0),
            )
            .optional()?;
        id.map(|id| self.get_job(&id)).transpose()
    }
    pub fn integrity_review(
        &self,
        transcript_id: &str,
        fingerprint: &str,
    ) -> Result<Option<(String, String)>> {
        Ok(self.connection()?.query_row("SELECT note,reviewed_at FROM integrity_reviews WHERE transcript_id=?1 AND fingerprint=?2",params![transcript_id,fingerprint],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
    }
    /// Historical summary only; this does not resolve individual current issues.
    pub fn latest_integrity_review(&self, transcript_id: &str) -> Result<Option<(String, String)>> {
        Ok(self.connection()?.query_row("SELECT note,reviewed_at FROM integrity_reviews WHERE transcript_id=?1 ORDER BY reviewed_at DESC, fingerprint DESC LIMIT 1",[transcript_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
    }
    pub fn save_integrity_review(
        &self,
        transcript_id: &str,
        fingerprint: &str,
        note: &str,
    ) -> Result<()> {
        ensure!(
            !note.trim().is_empty() && note.chars().count() <= 4000,
            "请填写 1–4000 字的核对结论"
        );
        ensure!(
            fingerprint.len() == 64 && fingerprint.chars().all(|c| c.is_ascii_hexdigit()),
            "检查依据标识无效"
        );
        self.connection()?.execute("INSERT INTO integrity_reviews(transcript_id,fingerprint,note,reviewed_at) VALUES(?1,?2,?3,?4) ON CONFLICT(transcript_id,fingerprint) DO UPDATE SET note=excluded.note,reviewed_at=excluded.reviewed_at",params![transcript_id,fingerprint,note.trim(),now()])?;
        Ok(())
    }

    pub(crate) fn connection(&self) -> Result<Connection> {
        let connection = Connection::open(&self.path).context("无法打开数据库")?;
        connection.busy_timeout(Duration::from_secs(10))?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        Ok(connection)
    }

    /// Update asset metadata. Transcript save/activation alone owns the active
    /// pointer, so an older metadata snapshot cannot erase an active version.
    pub fn upsert_asset(&self, asset: &Asset) -> Result<()> {
        ensure!(!asset.id.trim().is_empty(), "课程 ID 不能为空");
        sqlite_milliseconds(asset.duration_ms)?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(proposed) = &asset.active_version_id {
            let owned_version: bool = transaction.query_row(
                "SELECT EXISTS(SELECT 1 FROM transcripts WHERE id=?1 AND asset_id=?2)",
                params![proposed, asset.id],
                |row| row.get(0),
            )?;
            ensure!(owned_version, "元数据中的转写版本不属于该课程");
        }
        transaction.execute(
            "INSERT INTO assets (id, title, source_kind, source, bvid, page, duration_ms, audio_path, active_version_id, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,?10)
             ON CONFLICT(id) DO UPDATE SET title=excluded.title, source_kind=excluded.source_kind,
             source=excluded.source, bvid=excluded.bvid, page=excluded.page, duration_ms=excluded.duration_ms,
             audio_path=excluded.audio_path, updated_at=excluded.updated_at",
            params![asset.id, asset.title, asset.source_kind, asset.source, asset.bvid, asset.page,
                asset.duration_ms, asset.audio_path, asset.created_at, asset.updated_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_assets(&self) -> Result<Vec<Asset>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {ASSET_COLUMNS} FROM assets ORDER BY updated_at DESC, id"
        ))?;
        let records = statement
            .query_map([], asset_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(records)
    }

    pub fn get_asset(&self, id: &str) -> Result<Asset> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!("SELECT {ASSET_COLUMNS} FROM assets WHERE id=?1"),
                [id],
                asset_from_row,
            )
            .optional()?
            .with_context(|| format!("课程不存在：{id}"))
    }

    pub fn save_transcript(
        &self,
        asset_id: &str,
        source_kind: &str,
        model: Option<&str>,
        language: &str,
        segments: &[Segment],
    ) -> Result<Transcript> {
        self.save_transcript_inner(asset_id, source_kind, model, language, segments, None)
    }

    /// The result and terminal job state share one transaction. Retrying the
    /// same job never creates a duplicate version or reactivates an old result.
    pub fn save_job_transcript(
        &self,
        job_id: &str,
        source_kind: &str,
        model: Option<&str>,
        language: &str,
        segments: &[Segment],
    ) -> Result<Transcript> {
        let job = self.get_job(job_id)?;
        self.save_transcript_inner(
            &job.asset_id,
            source_kind,
            model,
            language,
            segments,
            Some(job_id),
        )
    }

    fn save_transcript_inner(
        &self,
        asset_id: &str,
        source_kind: &str,
        model: Option<&str>,
        language: &str,
        segments: &[Segment],
        job_id: Option<&str>,
    ) -> Result<Transcript> {
        validate_segments(segments)?;
        for segment in segments {
            sqlite_milliseconds(segment.start_ms)?;
            sqlite_milliseconds(segment.end_ms)?;
        }
        let indexed = index_text(segments);
        let mut connection = self.connection()?;
        // Acquire the write lock before finding the next number. Two callers
        // can never allocate the same version or partially replace the index.
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        if let Some(job_id) = job_id {
            let existing: Option<String> = transaction
                .query_row(
                    "SELECT transcript_id FROM job_transcripts WHERE job_id=?1",
                    [job_id],
                    |row| row.get(0),
                )
                .optional()?;
            if let Some(id) = existing {
                return load_transcript(&transaction, &id);
            }
            let running: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE id=?1 AND asset_id=?2 AND status='running')", params![job_id,asset_id], |row| row.get(0))?;
            ensure!(running, "任务已不在运行状态，不能提交结果");
        }
        let transcript = insert_transcript(
            &transaction,
            asset_id,
            source_kind,
            model,
            language,
            segments,
            &indexed,
        )?;
        if let Some(job_id) = job_id {
            transaction.execute(
                "INSERT INTO job_transcripts(job_id,transcript_id) VALUES(?1,?2)",
                params![job_id, transcript.id],
            )?;
            transaction.execute("UPDATE jobs SET status='completed',stage='已完成',progress=100,error=NULL,updated_at=?1 WHERE id=?2", params![transcript.created_at,job_id])?;
        }
        transaction.commit()?;
        Ok(transcript)
    }

    /// A multi-part import becomes visible as one batch, including conflicts
    /// discovered on its last part. Snapshot files are prepared by the caller.
    pub fn enqueue_jobs(&self, batch: &[(Asset, Job)]) -> Result<()> {
        let mut connection = self.connection()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (asset, job) in batch {
            ensure!(
                job.asset_id == asset.id && job.status == "queued",
                "任务与课程不匹配"
            );
            let busy: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM jobs WHERE asset_id=?1 AND status IN ('queued','running'))", [&asset.id], |row| row.get(0))?;
            ensure!(!busy, "该课程已有进行中的任务：{}", asset.title);
            sqlite_milliseconds(asset.duration_ms)?;
            tx.execute("INSERT INTO assets(id,title,source_kind,source,bvid,page,duration_ms,audio_path,active_version_id,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,?9,?10) ON CONFLICT(id) DO NOTHING", params![asset.id,asset.title,asset.source_kind,asset.source,asset.bvid,asset.page,asset.duration_ms,asset.audio_path,asset.created_at,asset.updated_at])?;
            tx.execute("INSERT INTO jobs(id,asset_id,title,status,stage,progress,error,model,device,mode,preset,chunk_done,chunk_total,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params![job.id,job.asset_id,job.title,job.status,job.stage,job.progress,job.error,job.model,job.device,job.mode,job.preset,job.chunk_done,job.chunk_total,job.created_at,job.updated_at])?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn get_transcript(&self, id: &str) -> Result<Transcript> {
        load_transcript(&self.connection()?, id)
    }

    pub fn active_transcript(&self, asset_id: &str) -> Result<Option<Transcript>> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let id: Option<Option<String>> = transaction
            .query_row(
                "SELECT active_version_id FROM assets WHERE id=?1",
                [asset_id],
                |row| row.get(0),
            )
            .optional()?;
        let transcript = id
            .flatten()
            .map(|id| load_transcript(&transaction, &id))
            .transpose()?;
        transaction.commit()?;
        Ok(transcript)
    }

    pub fn list_transcripts(&self, asset_id: &str) -> Result<Vec<Transcript>> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let ids = {
            let mut statement = transaction
                .prepare("SELECT id FROM transcripts WHERE asset_id=?1 ORDER BY version DESC")?;
            let ids = statement
                .query_map([asset_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            ids
        };
        let transcripts = ids
            .iter()
            .map(|id| load_transcript(&transaction, id))
            .collect::<Result<Vec<_>>>()?;
        transaction.commit()?;
        Ok(transcripts)
    }

    pub fn activate_transcript(&self, asset_id: &str, id: &str) -> Result<()> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transcript = load_transcript(&transaction, id)?;
        ensure!(transcript.asset_id == asset_id, "转写版本不属于该课程");
        let indexed = index_text(&transcript.segments);
        transaction.execute(
            "UPDATE assets SET active_version_id=?1, updated_at=?2 WHERE id=?3",
            params![id, now(), asset_id],
        )?;
        replace_index(&transaction, &transcript, &indexed)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn upsert_job(&self, job: &Job) -> Result<()> {
        ensure!(!job.id.trim().is_empty(), "任务 ID 不能为空");
        ensure!(
            job.progress.is_finite() && (0.0..=100.0).contains(&job.progress),
            "任务进度必须在 0 至 100 之间"
        );
        ensure!(
            job.chunk_done <= job.chunk_total,
            "已完成分块数不能大于总分块数"
        );
        ensure!(
            [
                "queued",
                "running",
                "paused",
                "completed",
                "failed",
                "cancelled"
            ]
            .contains(&job.status.as_str()),
            "无效的任务状态"
        );
        ensure!(
            ["auto", "subtitlesOnly", "transcribe"].contains(&job.mode.as_str()),
            "无效的任务模式"
        );
        ensure!(
            ["eco", "balanced", "quality", "custom"].contains(&job.preset.as_str()),
            "无效的任务预设"
        );
        self.connection()?.execute(
            "INSERT INTO jobs (id, asset_id, title, status, stage, progress, error, model, device, mode, preset, chunk_done, chunk_total, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)
             ON CONFLICT(id) DO UPDATE SET asset_id=excluded.asset_id, title=excluded.title, status=excluded.status,
             stage=excluded.stage, progress=excluded.progress, error=excluded.error, model=excluded.model, device=excluded.device,
             mode=excluded.mode, preset=excluded.preset, chunk_done=excluded.chunk_done, chunk_total=excluded.chunk_total,
             updated_at=excluded.updated_at",
            params![job.id, job.asset_id, job.title, job.status, job.stage, job.progress, job.error,
                job.model, job.device, job.mode, job.preset, job.chunk_done, job.chunk_total, job.created_at, job.updated_at],
        )?;
        Ok(())
    }

    pub fn get_job(&self, id: &str) -> Result<Job> {
        self.connection()?
            .query_row(
                &format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id=?1"),
                [id],
                job_from_row,
            )
            .optional()?
            .with_context(|| format!("任务不存在：{id}"))
    }

    pub fn list_jobs(&self) -> Result<Vec<Job>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs ORDER BY created_at DESC, id"
        ))?;
        let jobs = statement
            .query_map([], job_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(jobs)
    }

    /// Recovery changes only liveness and its timestamp. Model settings, stage,
    /// progress and completed chunk counts remain usable by checkpoint resume.
    pub fn recover_jobs(&self) -> Result<usize> {
        Ok(self.connection()?.execute(
            "UPDATE jobs SET status='paused', updated_at=?1 WHERE status IN ('running','queued')",
            [now()],
        )?)
    }

    pub fn save_note(&self, note: &Note) -> Result<()> {
        ensure!(!note.id.trim().is_empty(), "笔记 ID 不能为空");
        ensure!(
            ["summary", "answer", "manual"].contains(&note.kind.as_str()),
            "无效的笔记类型"
        );
        let mut connection = self.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let transcript = load_transcript(&transaction, &note.transcript_id)?;
        ensure!(
            transcript.asset_id == note.asset_id,
            "笔记绑定的转写不属于该课程"
        );
        let source: HashMap<_, _> = transcript
            .segments
            .iter()
            .map(|segment| (segment.id.as_str(), segment))
            .collect();
        for citation in &note.citations {
            let original = source
                .get(citation.segment_id.as_str())
                .context("笔记包含未知字幕引用")?;
            ensure!(
                citation.start_ms == original.start_ms
                    && citation.end_ms == original.end_ms
                    && citation.text == original.text,
                "笔记引用与原始字幕片段不一致"
            );
        }
        let citations = serde_json::to_string(&note.citations)?;
        transaction.execute(
            "INSERT INTO notes (id, asset_id, transcript_id, kind, title, content, citations_json, question, created_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)
             ON CONFLICT(id) DO UPDATE SET asset_id=excluded.asset_id, transcript_id=excluded.transcript_id,
             kind=excluded.kind, title=excluded.title, content=excluded.content, citations_json=excluded.citations_json, question=excluded.question",
            params![note.id, note.asset_id, note.transcript_id, note.kind, note.title, note.content, citations, note.question, note.created_at],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn list_notes(&self, asset_id: &str) -> Result<Vec<Note>> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT n.id, n.asset_id, n.transcript_id, n.kind, n.title, n.content, n.citations_json, n.question, n.created_at,
             CASE WHEN a.active_version_id=n.transcript_id THEN 0 ELSE 1 END AS stale
             FROM notes n JOIN assets a ON a.id=n.asset_id WHERE n.asset_id=?1 ORDER BY n.created_at DESC, n.id",
        )?;
        let notes = statement
            .query_map([asset_id], note_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(notes)
    }

    pub fn search(&self, query: &str, asset_id: Option<&str>) -> Result<Vec<SearchHit>> {
        let terms = tokenize(query);
        if terms.is_empty() {
            return Ok(Vec::new());
        }
        // Quoting every term makes user text literal FTS input. In particular,
        // quotes, OR, column selectors and wildcards cannot alter the query.
        let expression = terms
            .iter()
            .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT f.asset_id, a.title, f.transcript_id, f.segment_id, f.start_ms, f.end_ms, f.text, -bm25(segment_fts)
             FROM segment_fts f JOIN assets a ON a.id=f.asset_id AND a.active_version_id=f.transcript_id
             WHERE segment_fts MATCH ?1 AND (?2 IS NULL OR f.asset_id=?2)
             ORDER BY bm25(segment_fts), a.id, f.start_ms, f.segment_id",
        )?;
        let hits = statement
            .query_map(params![expression, asset_id], |row| {
                Ok(SearchHit {
                    asset_id: row.get(0)?,
                    asset_title: row.get(1)?,
                    transcript_id: row.get(2)?,
                    segment_id: row.get(3)?,
                    start_ms: row.get(4)?,
                    end_ms: row.get(5)?,
                    text: row.get(6)?,
                    score: row.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(hits)
    }
}

fn asset_from_row(row: &Row<'_>) -> rusqlite::Result<Asset> {
    Ok(Asset {
        id: row.get(0)?,
        title: row.get(1)?,
        source_kind: row.get(2)?,
        source: row.get(3)?,
        bvid: row.get(4)?,
        page: row.get(5)?,
        duration_ms: row.get(6)?,
        audio_path: row.get(7)?,
        active_version_id: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        title: row.get(2)?,
        status: row.get(3)?,
        stage: row.get(4)?,
        progress: row.get(5)?,
        error: row.get(6)?,
        model: row.get(7)?,
        device: row.get(8)?,
        mode: row.get(9)?,
        preset: row.get(10)?,
        chunk_done: row.get(11)?,
        chunk_total: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn note_from_row(row: &Row<'_>) -> rusqlite::Result<Note> {
    let json: String = row.get(6)?;
    let citations: Vec<Citation> = serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(6, Type::Text, Box::new(error))
    })?;
    Ok(Note {
        id: row.get(0)?,
        asset_id: row.get(1)?,
        transcript_id: row.get(2)?,
        kind: row.get(3)?,
        title: row.get(4)?,
        content: row.get(5)?,
        citations,
        question: row.get(7)?,
        created_at: row.get(8)?,
        stale: row.get(9)?,
    })
}

pub(crate) fn load_transcript(connection: &Connection, id: &str) -> Result<Transcript> {
    let mut transcript = connection
        .query_row(
            "SELECT t.id, t.asset_id, t.version, t.source_kind, t.model, t.language, t.created_at,
         CASE WHEN a.active_version_id=t.id THEN 1 ELSE 0 END
         FROM transcripts t JOIN assets a ON a.id=t.asset_id WHERE t.id=?1",
            [id],
            |row| {
                Ok(Transcript {
                    id: row.get(0)?,
                    asset_id: row.get(1)?,
                    version: row.get(2)?,
                    source_kind: row.get(3)?,
                    model: row.get(4)?,
                    language: row.get(5)?,
                    segments: Vec::new(),
                    created_at: row.get(6)?,
                    is_active: row.get(7)?,
                })
            },
        )
        .optional()?
        .with_context(|| format!("转写版本不存在：{id}"))?;
    let mut statement = connection.prepare(
        "SELECT id, start_ms, end_ms, text FROM segments WHERE transcript_id=?1 ORDER BY ordinal",
    )?;
    transcript.segments = statement
        .query_map([id], |row| {
            Ok(Segment {
                id: row.get(0)?,
                start_ms: row.get(1)?,
                end_ms: row.get(2)?,
                text: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(transcript)
}

fn replace_index(
    connection: &Connection,
    transcript: &Transcript,
    indexed: &[String],
) -> Result<()> {
    connection.execute(
        "DELETE FROM segment_fts WHERE asset_id=?1",
        [&transcript.asset_id],
    )?;
    let mut statement = connection.prepare(
        "INSERT INTO segment_fts (asset_id, transcript_id, segment_id, start_ms, end_ms, text, tokens) VALUES (?1,?2,?3,?4,?5,?6,?7)",
    )?;
    for (segment, tokens) in transcript.segments.iter().zip(indexed) {
        statement.execute(params![
            transcript.asset_id,
            transcript.id,
            segment.id,
            segment.start_ms,
            segment.end_ms,
            segment.text,
            tokens
        ])?;
    }
    Ok(())
}

pub(crate) fn index_text(segments: &[Segment]) -> Vec<String> {
    segments
        .iter()
        .map(|segment| tokenize(&segment.text).join(" "))
        .collect()
}

fn tokenize(text: &str) -> Vec<String> {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    let jieba = JIEBA.get_or_init(Jieba::new);
    let mut seen = HashSet::new();
    jieba
        .cut_for_search(text, true)
        .into_iter()
        .filter(|token| token.chars().any(char::is_alphanumeric))
        .map(str::to_lowercase)
        .filter(|token| seen.insert(token.clone()))
        .collect()
}

fn sqlite_milliseconds(value: u64) -> Result<i64> {
    i64::try_from(value).context("毫秒数超出 SQLite 可支持的范围")
}

pub(crate) fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub(crate) fn insert_transcript(
    connection: &Connection,
    asset_id: &str,
    source_kind: &str,
    model: Option<&str>,
    language: &str,
    segments: &[Segment],
    indexed: &[String],
) -> Result<Transcript> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM assets WHERE id=?1)",
        [asset_id],
        |row| row.get(0),
    )?;
    ensure!(exists, "课程不存在：{asset_id}");
    let previous: u32 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM transcripts WHERE asset_id=?1",
        [asset_id],
        |row| row.get(0),
    )?;
    let version = previous.checked_add(1).context("转写版本数量超出上限")?;
    let transcript = Transcript {
        id: Uuid::new_v4().to_string(),
        asset_id: asset_id.into(),
        version,
        source_kind: source_kind.into(),
        model: model.map(str::to_owned),
        language: language.into(),
        segments: segments.to_vec(),
        created_at: now(),
        is_active: true,
    };
    connection.execute(
            "INSERT INTO transcripts (id, asset_id, version, source_kind, model, language, created_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",
            params![transcript.id, transcript.asset_id, transcript.version, transcript.source_kind,
                transcript.model, transcript.language, transcript.created_at],
        )?;
    {
        let mut statement = connection.prepare(
                "INSERT INTO segments (transcript_id, id, ordinal, start_ms, end_ms, text) VALUES (?1,?2,?3,?4,?5,?6)",
            )?;
        for (ordinal, segment) in segments.iter().enumerate() {
            statement.execute(params![
                transcript.id,
                segment.id,
                ordinal as i64,
                segment.start_ms,
                segment.end_ms,
                segment.text
            ])?;
        }
    }
    connection.execute(
        "UPDATE assets SET active_version_id=?1, updated_at=?2 WHERE id=?3",
        params![transcript.id, transcript.created_at, asset_id],
    )?;
    replace_index(connection, &transcript, indexed)?;
    Ok(transcript)
}
