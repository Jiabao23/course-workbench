use super::{
    asr::{transcribe_request, AsrEngine, WhisperWorker},
    benchmark::{self, BenchmarkRecord},
    cache,
    knowledge::{KnowledgeProvider, OpenAiCompatible},
    media,
    process::ProcessControl,
    profiler::{LocalProfiler, ResourceProfiler, ResourceReport},
    settings::{self, AppSettings},
    source::{self, BilibiliProvider, SourcePart, SourcePreview, SourceProvider},
    web_source::WebProvider,
};
use anyhow::{anyhow, bail, ensure, Context, Result};
use chrono::{SecondsFormat, Utc};
use course_core::{
    Asset, Citation, Db, Job, Note, ResourceRecommendation, SearchHit, Segment, SystemResources,
    Transcript,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex, RwLock,
    },
};
use uuid::Uuid;

fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}
fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn lock_library(settings: &AppSettings) -> Result<fs::File> {
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(settings.data_path().join("library.lock"))?;
    lock.try_lock()
        .context("资料库正由另一个应用或诊断进程使用，请先关闭该进程")?;
    Ok(lock)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub assets: Vec<Asset>,
    pub jobs: Vec<Job>,
    pub settings: AppSettings,
    pub resources: SystemResources,
    pub recommendation: ResourceRecommendation,
    pub api_key_configured: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetDetail {
    pub asset: Asset,
    pub transcript: Option<Transcript>,
    pub versions: Vec<Transcript>,
    pub notes: Vec<Note>,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateJobsRequest {
    pub source: String,
    pub pages: Vec<u32>,
    pub mode: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JobSnapshot {
    settings: AppSettings,
    part: SourcePart,
    audio_only: bool,
    #[serde(default)]
    fallback_used: bool,
}

type EventSink = Arc<dyn Fn(Job) + Send + Sync>;
pub struct Runtime {
    pub config_path: PathBuf,
    pub worker_path: PathBuf,
    settings: RwLock<AppSettings>,
    database: RwLock<Db>,
    library_lock: Mutex<fs::File>,
    report: Mutex<Option<ResourceReport>>,
    controls: Mutex<HashMap<String, Arc<ProcessControl>>>,
    event_sink: Mutex<Option<EventSink>>,
    worker_active: AtomicBool,
    auxiliary_active: AtomicBool,
    shutting_down: AtomicBool,
    mutation_gate: Mutex<()>,
    config_revision: AtomicU64,
}
struct AuxiliaryGuard<'a> {
    runtime: &'a Runtime,
    id: String,
    control: Arc<ProcessControl>,
    exclusive: bool,
}
impl Drop for AuxiliaryGuard<'_> {
    fn drop(&mut self) {
        self.runtime.controls.lock().unwrap().remove(&self.id);
        if self.exclusive {
            self.runtime.auxiliary_active.store(false, Ordering::SeqCst);
        }
    }
}

impl Runtime {
    pub fn new(config_path: PathBuf, worker_path: PathBuf) -> Result<Arc<Self>> {
        let settings = settings::load(&config_path)?;
        let library_lock = lock_library(&settings)?;
        let database = Db::open(&settings.data_path().join("library.sqlite3"))?;
        database.recover_jobs()?;
        Ok(Arc::new(Self {
            config_path,
            worker_path,
            settings: RwLock::new(settings),
            database: RwLock::new(database),
            library_lock: Mutex::new(library_lock),
            report: Mutex::new(None),
            controls: Mutex::new(HashMap::new()),
            event_sink: Mutex::new(None),
            worker_active: AtomicBool::new(false),
            auxiliary_active: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
            mutation_gate: Mutex::new(()),
            config_revision: AtomicU64::new(0),
        }))
    }
    pub fn settings(&self) -> AppSettings {
        self.settings.read().unwrap().clone()
    }
    pub fn db(&self) -> Db {
        self.database.read().unwrap().clone()
    }
    pub fn is_busy(&self) -> bool {
        self.worker_active.load(Ordering::SeqCst) || self.auxiliary_active.load(Ordering::SeqCst)
    }
    pub fn set_event_sink(&self, sink: EventSink) {
        *self.event_sink.lock().unwrap() = Some(sink);
    }
    fn publish(&self, job: &mut Job) -> Result<()> {
        job.updated_at = now();
        self.db().upsert_job(job)?;
        let sink = self.event_sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            sink(job.clone());
        }
        Ok(())
    }
    pub fn probe_resources(&self) -> Result<ResourceReport> {
        let revision = self.config_revision.load(Ordering::SeqCst);
        let report = LocalProfiler {
            worker_path: self.worker_path.clone(),
        }
        .profile(&self.settings())?;
        let mut cached = self.report.lock().unwrap();
        if revision == self.config_revision.load(Ordering::SeqCst) {
            *cached = Some(report.clone());
        }
        Ok(report)
    }
    fn current_report(&self) -> Result<ResourceReport> {
        let cached = self.report.lock().unwrap().clone();
        match cached {
            Some(report) => Ok(report),
            None => self.probe_resources(),
        }
    }
    pub fn bootstrap(&self) -> Result<Bootstrap> {
        let mut report = self.current_report()?;
        let api_key_configured = match self.api_key() {
            Ok(key) => key.is_some(),
            Err(_) => {
                report.resources.warnings.push("系统凭据暂时不可用；本地文字稿、搜索与手写笔记仍可使用。请在使用云端 API 前检查凭据存储。".into());
                false
            }
        };
        let _gate = self.mutation_gate.lock().unwrap();
        Ok(Bootstrap {
            assets: self.db().list_assets()?,
            jobs: self.db().list_jobs()?,
            settings: self.settings(),
            resources: report.resources,
            recommendation: report.recommendation,
            api_key_configured,
        })
    }
    pub fn probe_source(&self, input: &str) -> Result<SourcePreview> {
        let guard = self.begin_source_probe()?;
        let settings = self.settings();
        if source::is_bilibili_source(input) {
            BilibiliProvider::new(&settings.cookie_file)?.preview(input)
        } else if input.trim().contains("://") {
            WebProvider::new(&settings, &guard.control).preview(input)
        } else {
            let mut preview = source::local_preview(input)?;
            if preview.source_kind == "localMedia" && !settings.ffprobe_path.is_empty() {
                preview.parts[0].duration_ms =
                    media::duration_ms(&settings, Path::new(&preview.source), &guard.control)?;
            }
            Ok(preview)
        }
    }
    fn begin_source_probe(&self) -> Result<AuxiliaryGuard<'_>> {
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(!self.shutting_down.load(Ordering::SeqCst), "应用正在退出");
        let id = format!("probe-{}", new_id());
        let control = Arc::new(ProcessControl::default());
        self.controls
            .lock()
            .unwrap()
            .insert(id.clone(), control.clone());
        Ok(AuxiliaryGuard {
            runtime: self,
            id,
            control,
            exclusive: false,
        })
    }
    pub fn probe_part(&self, input: &str, page: u32) -> Result<SourcePart> {
        if !source::is_bilibili_source(input) {
            return self
                .probe_source(input)?
                .parts
                .into_iter()
                .find(|part| part.page == page)
                .context("不存在该处理项");
        }
        let provider = BilibiliProvider::new(&self.settings().cookie_file)?;
        let preview = provider.preview(input)?;
        let mut part = preview
            .parts
            .into_iter()
            .find(|p| p.page == page)
            .context("不存在该分 P")?;
        provider.inspect_part(preview.bvid.as_deref().context("缺少 BV 号")?, &mut part)?;
        Ok(part)
    }
    fn resolved_settings(&self) -> Result<AppSettings> {
        let mut settings = self.settings();
        // Available RAM/VRAM can change while the app stays open.
        let report = self.probe_resources()?;
        if settings.preset != "custom" {
            let choice = course_core::resources::recommend(&report.resources, &settings.preset);
            settings.model = choice.model;
            settings.device = choice.device;
            settings.threads = choice.threads;
        } else if settings.device == "auto" {
            settings.device = if report.resources.cuda_available {
                "cuda"
            } else {
                "cpu"
            }
            .into();
        }
        Ok(settings)
    }
    pub fn create_jobs(self: &Arc<Self>, request: CreateJobsRequest) -> Result<Vec<Job>> {
        let revision = self.config_revision.load(Ordering::SeqCst);
        ensure!(
            ["auto", "subtitlesOnly", "transcribe"].contains(&request.mode.as_str()),
            "未知处理模式"
        );
        ensure!(!request.pages.is_empty(), "请至少选择一个分 P");
        ensure!(request.pages.len() <= 100, "一次最多添加 100 个分 P");
        let unique: HashSet<_> = request.pages.iter().copied().collect();
        ensure!(unique.len() == request.pages.len(), "选择的分 P 重复");
        let preview = self.probe_source(&request.source)?;
        let selected: Result<Vec<_>> = request
            .pages
            .iter()
            .map(|page| {
                preview
                    .parts
                    .iter()
                    .find(|part| part.page == *page)
                    .cloned()
                    .context("选择的分 P 不存在")
            })
            .collect();
        let selected = selected?;
        ensure!(
            preview.source_kind != "subtitle" || request.mode != "transcribe",
            "字幕文件没有音轨；请使用字幕导入模式"
        );
        let settings = self.resolved_settings()?;
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(
            revision == self.config_revision.load(Ordering::SeqCst),
            "配置已改变，请重新读取课程后添加任务"
        );
        ensure!(!self.shutting_down.load(Ordering::SeqCst), "应用正在退出");
        ensure!(
            !self.auxiliary_active.load(Ordering::SeqCst),
            "正在运行环境操作，请完成后再添加任务"
        );
        let db = self.db();
        let existing = db.list_assets()?;
        let jobs = db.list_jobs()?;
        let mut batch = vec![];
        for part in selected {
            let timestamp = now();
            let canonical_source = if let Some(bvid) = &preview.bvid {
                format!("https://www.bilibili.com/video/{bvid}?p={}", part.page)
            } else {
                preview.source.clone()
            };
            let asset = existing
                .iter()
                .find(|asset| {
                    asset.source == canonical_source
                        || (matches!(preview.source_kind.as_str(), "localMedia" | "subtitle")
                            && asset.source_kind == preview.source_kind
                            && dunce::simplified(Path::new(&asset.source))
                                == dunce::simplified(Path::new(&canonical_source)))
                })
                .cloned()
                .unwrap_or_else(|| Asset {
                    id: new_id(),
                    title: if preview.parts.len() > 1 {
                        format!("{} · P{} {}", preview.title, part.page, part.title)
                    } else {
                        preview.title.clone()
                    },
                    source_kind: preview.source_kind.clone(),
                    source: canonical_source,
                    bvid: preview.bvid.clone(),
                    page: Some(part.page),
                    duration_ms: part.duration_ms,
                    audio_path: None,
                    active_version_id: None,
                    created_at: timestamp.clone(),
                    updated_at: timestamp.clone(),
                });
            ensure!(
                !jobs.iter().any(|job| job.asset_id == asset.id
                    && ["queued", "running"].contains(&job.status.as_str())),
                "该课程已有进行中的任务"
            );
            let job = Job {
                id: new_id(),
                asset_id: asset.id.clone(),
                title: asset.title.clone(),
                status: "queued".into(),
                stage: "等待处理".into(),
                progress: 0.0,
                error: None,
                model: settings.model.clone(),
                device: settings.device.clone(),
                mode: request.mode.clone(),
                preset: settings.preset.clone(),
                chunk_done: 0,
                chunk_total: 0,
                created_at: timestamp.clone(),
                updated_at: timestamp,
            };
            self.save_snapshot(
                &job.id,
                &JobSnapshot {
                    settings: settings.clone(),
                    part,
                    audio_only: false,
                    fallback_used: false,
                },
            )?;
            batch.push((asset, job));
        }
        db.enqueue_jobs(&batch)?;
        let created: Vec<_> = batch.into_iter().map(|(_, job)| job).collect();
        let sink = self.event_sink.lock().unwrap().clone();
        if let Some(sink) = sink {
            for job in &created {
                sink(job.clone());
            }
        }
        drop(_gate);
        self.kick();
        Ok(created)
    }
    fn snapshot_path(&self, job_id: &str) -> PathBuf {
        self.settings()
            .data_path()
            .join("jobs")
            .join(format!("{job_id}.json"))
    }
    fn save_snapshot(&self, job_id: &str, snapshot: &JobSnapshot) -> Result<()> {
        settings::atomic_write(
            &self.snapshot_path(job_id),
            &serde_json::to_vec_pretty(snapshot)?,
        )
    }
    fn read_snapshot(&self, job_id: &str) -> Result<JobSnapshot> {
        serde_json::from_slice(&fs::read(self.snapshot_path(job_id))?)
            .context("任务配置快照损坏，请重新导入")
    }
    fn kick(self: &Arc<Self>) {
        if self.shutting_down.load(Ordering::SeqCst)
            || self.worker_active.swap(true, Ordering::SeqCst)
        {
            return;
        }
        let runtime = self.clone();
        std::thread::spawn(move || {
            loop {
                if runtime.shutting_down.load(Ordering::SeqCst) {
                    break;
                }
                let next = runtime
                    .db()
                    .list_jobs()
                    .ok()
                    .and_then(|jobs| jobs.into_iter().rev().find(|job| job.status == "queued"));
                let Some(job) = next else {
                    break;
                };
                runtime.execute_job(job);
            }
            runtime.worker_active.store(false, Ordering::SeqCst);
            if !runtime.shutting_down.load(Ordering::SeqCst)
                && runtime
                    .db()
                    .list_jobs()
                    .is_ok_and(|jobs| jobs.iter().any(|job| job.status == "queued"))
            {
                runtime.kick();
            }
        });
    }
    fn execute_job(&self, mut job: Job) {
        let control = Arc::new(ProcessControl::default());
        {
            let _gate = self.mutation_gate.lock().unwrap();
            if self
                .db()
                .get_job(&job.id)
                .is_ok_and(|current| current.status != "queued")
            {
                return;
            }
            self.controls
                .lock()
                .unwrap()
                .insert(job.id.clone(), control.clone());
            job.status = "running".into();
            job.stage = "准备处理".into();
            job.error = None;
            if self.publish(&mut job).is_err() {
                self.controls.lock().unwrap().remove(&job.id);
                return;
            }
        }
        let result = (|| {
            let mut snapshot = self.read_snapshot(&job.id)?;
            match self.process_job(&mut job, &snapshot, &control) {
                Err(error)
                    if error.to_string().contains("out_of_memory")
                        && !control.cancelled.load(Ordering::SeqCst) =>
                {
                    if let Some(profile) = self.smaller_validated_profile(&job, &snapshot)? {
                        snapshot.settings.model = profile.model.clone();
                        snapshot.settings.device = profile.device.clone();
                        snapshot.settings.threads = profile.threads;
                        snapshot.fallback_used = true;
                        self.save_snapshot(&job.id, &snapshot)?;
                        job.model = profile.model;
                        job.device = profile.device;
                        job.chunk_done = 0;
                        job.chunk_total = 0;
                        job.progress = 0.0;
                        job.stage = "显存不足，改用已验证的较小配置，重新生成转写版本".into();
                        self.publish(&mut job)?;
                        self.process_job(&mut job, &snapshot, &control)
                    } else {
                        Err(anyhow!("显存不足，已完成分块保留。没有可自动切换的已验证配置；请调整设置后使用‘按当前配置重试’，新模型会生成独立版本。\n{error}"))
                    }
                }
                other => other,
            }
        })();
        // Cancellation and terminal publication share the same critical
        // section; a stale running snapshot can never overwrite completion.
        let _gate = self.mutation_gate.lock().unwrap();
        self.controls.lock().unwrap().remove(&job.id);
        match result {
            Ok(()) => {
                job.status = "completed".into();
                job.stage = "已完成".into();
                job.progress = 100.0;
                job.error = None;
            }
            Err(error) => {
                if self.shutting_down.load(Ordering::SeqCst) {
                    job.status = "paused".into();
                    job.stage = "应用退出，等待恢复".into();
                } else if control.cancelled.load(Ordering::SeqCst) {
                    job.status = "cancelled".into();
                    job.stage = "已取消，可从检查点重试".into();
                } else {
                    job.status = "failed".into();
                    job.stage = "处理失败".into();
                }
                job.error = Some(format!("{error:#}"));
            }
        }
        let _ = self.publish(&mut job);
    }
    fn process_job(
        &self,
        job: &mut Job,
        snapshot: &JobSnapshot,
        control: &ProcessControl,
    ) -> Result<()> {
        let db = self.db();
        let mut asset = db.get_asset(&job.asset_id)?;
        let settings = &snapshot.settings;
        control.check()?;
        if !snapshot.audio_only && job.mode != "transcribe" {
            if asset.source_kind == "subtitle" {
                job.stage = "读取字幕".into();
                self.publish(job)?;
                let path = Path::new(&asset.source);
                let format = path.extension().and_then(|s| s.to_str()).unwrap_or("srt");
                let segments =
                    course_core::subtitles::parse_subtitles(&fs::read_to_string(path)?, format)?;
                self.commit_transcript(
                    job,
                    "subtitle",
                    None,
                    &settings.language,
                    &segments,
                    control,
                )?;
                return Ok(());
            }
            if matches!(asset.source_kind.as_str(), "bilibili" | "webMedia") {
                job.stage = "检查来源字幕".into();
                self.publish(job)?;
                let provider: Box<dyn SourceProvider + '_> = if asset.source_kind == "bilibili" {
                    Box::new(BilibiliProvider::new(&settings.cookie_file)?)
                } else {
                    Box::new(WebProvider::new(settings, control))
                };
                let mut part = snapshot.part.clone();
                provider.inspect_part(asset.bvid.as_deref().unwrap_or(&asset.source), &mut part)?;
                control.check()?;
                match part.subtitle_status.as_str() {
                    "available" => {
                        let track = part
                            .subtitles
                            .iter()
                            .find(|track| track.language.starts_with(&settings.language))
                            .or_else(|| part.subtitles.first())
                            .context("字幕列表为空")?;
                        job.stage = "提取可用字幕，无需下载音轨".into();
                        self.publish(job)?;
                        let segments = provider.subtitles(&asset.source, track)?;
                        control.check()?;
                        self.commit_transcript(
                            job,
                            if asset.source_kind == "bilibili" {
                                "bilibiliSubtitle"
                            } else {
                                "webSubtitle"
                            },
                            None,
                            &track.language,
                            &segments,
                            control,
                        )?;
                        return Ok(());
                    }
                    "failed" | "unchecked" => {
                        bail!("字幕接口请求失败，请稍后重试；也可明确选择强制转写")
                    }
                    "loginRequired" if job.mode == "subtitlesOnly" => {
                        bail!("字幕需要登录。请在设置中配置本人导出的 Cookie，或选择自动转写。")
                    }
                    "absent" if job.mode == "subtitlesOnly" => {
                        bail!("所选来源没有可读取字幕，‘仅提取字幕’不会下载媒体或转写。")
                    }
                    "loginRequired" => {
                        job.stage = "字幕需要登录，准备获取音轨转写".into();
                        self.publish(job)?;
                    }
                    _ => (),
                }
            } else if job.mode == "subtitlesOnly" {
                bail!("本地音视频没有独立字幕文件，请导入字幕文件或使用转写模式");
            }
        }
        if !snapshot.audio_only {
            ensure!(
                !settings.python_path.is_empty(),
                "请在设置中配置 Python/Whisper 环境"
            );
            let canonical_model = match job.model.as_str() {
                "large" => "large-v3",
                "turbo" => "large-v3-turbo",
                model => model,
            };
            ensure!(
                Path::new(&settings.model_dir)
                    .join(format!("{canonical_model}.pt"))
                    .is_file(),
                "模型 {} 尚未下载。请在设置中选择该模型并下载，再重试任务。",
                job.model
            );
        }
        let free = super::profiler::basic_resources(settings).disk_free_mb;
        let required = 512 + asset.duration_ms.saturating_mul(48) / 1024 / 1024;
        ensure!(
            free == 0 || free > required,
            "磁盘空间不足，预计至少需要 {required} MB 可用空间"
        );
        let audio = media::ensure_audio(settings, &asset, control, |stage, progress| {
            job.stage = if stage == "download" {
                if asset.source_kind == "webMedia" {
                    "获取来源媒体"
                } else {
                    "只下载音轨"
                }
            } else {
                "转换回听音频"
            }
            .into();
            job.progress = if stage == "download" {
                progress * 0.15
            } else {
                15.0 + progress * 0.1
            };
            self.publish(job)
        })?;
        asset.audio_path = Some(audio.to_string_lossy().into_owned());
        asset.duration_ms = media::duration_ms(settings, &audio, control)?;
        asset.updated_at = now();
        db.upsert_asset(&asset)?;
        if snapshot.audio_only {
            return Ok(());
        }
        let worker = WhisperWorker {
            path: self.worker_path.clone(),
        };
        let request = transcribe_request(
            settings,
            &job.id,
            &audio,
            &job.model,
            &job.device,
            &settings.cache_path("checkpoints"),
        );
        let result = worker.execute(settings, request, control, &mut |message| {
            if message["type"] == "progress" {
                job.progress = 25.0 + message["progress"].as_f64().unwrap_or(0.0) * 0.74;
                job.chunk_done = message["chunk_done"].as_u64().unwrap_or(0) as u32;
                job.chunk_total = message["chunk_total"].as_u64().unwrap_or(0) as u32;
                job.stage = match message["stage"].as_str() {
                    Some("verify_model") => "校验本地模型",
                    Some("load_model") => "加载识别模型",
                    _ => "转写音频，按分块保存检查点",
                }
                .into();
                self.publish(job)?;
            }
            Ok(())
        })?;
        control.check()?;
        let segments: Result<Vec<_>> = result["segments"]
            .as_array()
            .context("识别结果缺少片段")?
            .iter()
            .map(|s| {
                Ok(Segment {
                    id: s["id"].as_str().context("片段 ID 缺失")?.into(),
                    start_ms: s["start_ms"].as_u64().context("开始时间无效")?,
                    end_ms: s["end_ms"].as_u64().context("结束时间无效")?,
                    text: s["text"].as_str().context("片段文字无效")?.into(),
                })
            })
            .collect();
        let segments = segments?;
        super::integrity::require_complete_chunks(
            job.chunk_done,
            job.chunk_total,
            asset.duration_ms,
        )?;
        ensure!(
            !segments.is_empty(),
            "音频中未识别出有效语音，已有文字版本已保留"
        );
        self.commit_transcript(
            job,
            "whisper",
            Some(&job.model),
            &settings.language,
            &segments,
            control,
        )?;
        if result["resumed_chunks"].as_u64().unwrap_or(0) == 0 {
            // Metrics cannot turn a durably completed transcript into a failed
            // task. A full disk here must not cause duplicate versions on retry.
            let metrics = (|| -> Result<()> {
                let record = benchmark::from_result(
                    settings,
                    &self.current_report()?,
                    &asset.id,
                    &job.model,
                    &job.device,
                    asset.duration_ms as f64 / 1000.0,
                    &result,
                );
                benchmark::save(settings, &record)
            })();
            if let Err(error) = metrics {
                eprintln!("无法保存性能记录：{error:#}");
            }
        }
        Ok(())
    }
    fn commit_transcript(
        &self,
        job: &Job,
        source_kind: &str,
        model: Option<&str>,
        language: &str,
        segments: &[Segment],
        control: &ProcessControl,
    ) -> Result<Transcript> {
        let _gate = self.mutation_gate.lock().unwrap();
        control.check()?;
        self.db()
            .save_job_transcript(&job.id, source_kind, model, language, segments)
    }
    fn smaller_validated_profile(
        &self,
        job: &Job,
        snapshot: &JobSnapshot,
    ) -> Result<Option<BenchmarkRecord>> {
        if snapshot.fallback_used {
            return Ok(None);
        }
        let report = self.current_report()?;
        let fingerprint = benchmark::fingerprint(&snapshot.settings, &report);
        let rank = |model: &str| match model.split('.').next().unwrap_or(model) {
            "tiny" => 0,
            "base" => 1,
            "small" => 2,
            "medium" => 3,
            "turbo" | "large-v3-turbo" => 4,
            _ => 5,
        };
        Ok(benchmark::list(&snapshot.settings)?
            .into_iter()
            .find(|profile| {
                profile.success
                    && profile.tested
                    && profile.environment_fingerprint == fingerprint
                    && ((profile.device == job.device && rank(&profile.model) < rank(&job.model))
                        || (profile.device == "cpu"
                            && job.device == "cuda"
                            && rank(&profile.model) <= rank(&job.model)))
                    && Path::new(&snapshot.settings.model_dir)
                        .join(format!("{}.pt", profile.model))
                        .is_file()
            }))
    }
    pub fn cancel_job(&self, id: &str) -> Result<Job> {
        let _gate = self.mutation_gate.lock().unwrap();
        let mut job = self.db().get_job(id)?;
        ensure!(
            ["queued", "running", "paused"].contains(&job.status.as_str()),
            "该任务不在可取消状态"
        );
        if let Some(control) = self.controls.lock().unwrap().get(id) {
            control.cancel();
        }
        if job.status != "running" {
            job.status = "cancelled".into();
            job.stage = "已取消，可重试".into();
            self.publish(&mut job)?;
        } else {
            job.stage = "正在取消，保留已完成分块".into();
            self.publish(&mut job)?;
        }
        Ok(job)
    }
    pub fn retry_job(self: &Arc<Self>, id: &str, use_current_settings: bool) -> Result<Job> {
        let revision = self.config_revision.load(Ordering::SeqCst);
        let new_settings = if use_current_settings {
            Some(self.resolved_settings()?)
        } else {
            None
        };
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(
            revision == self.config_revision.load(Ordering::SeqCst),
            "配置已改变，请重新选择重试"
        );
        ensure!(
            !self.auxiliary_active.load(Ordering::SeqCst),
            "请等待环境操作完成"
        );
        let mut job = self.db().get_job(id)?;
        ensure!(
            ["failed", "cancelled", "paused"].contains(&job.status.as_str()),
            "该任务不需要重试"
        );
        ensure!(
            !self
                .db()
                .list_jobs()?
                .iter()
                .any(|other| other.asset_id == job.asset_id
                    && ["queued", "running"].contains(&other.status.as_str())),
            "该课程已有进行中的任务"
        );
        if let Some(settings) = new_settings {
            let mut snapshot = self.read_snapshot(id)?;
            snapshot.settings = settings.clone();
            snapshot.fallback_used = false;
            self.save_snapshot(id, &snapshot)?;
            job.model = settings.model;
            job.device = settings.device;
            job.preset = settings.preset;
            job.chunk_done = 0;
            job.chunk_total = 0;
            job.progress = 0.0;
        }
        job.status = "queued".into();
        job.stage = "等待恢复".into();
        job.error = None;
        self.publish(&mut job)?;
        drop(_gate);
        self.kick();
        Ok(job)
    }
    pub fn ensure_audio(self: &Arc<Self>, asset_id: &str) -> Result<Job> {
        let revision = self.config_revision.load(Ordering::SeqCst);
        let asset = self.db().get_asset(asset_id)?;
        ensure!(
            asset.source_kind != "subtitle",
            "独立字幕文件没有对应音轨，请另行导入音视频"
        );
        let mut preview = self.probe_source(&asset.source)?;
        let part = preview
            .parts
            .drain(..)
            .find(|part| Some(part.page) == asset.page)
            .context("找不到原分 P")?;
        let settings = self.resolved_settings()?;
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(
            revision == self.config_revision.load(Ordering::SeqCst),
            "资料库配置已改变，请重新打开课程"
        );
        ensure!(
            !self.auxiliary_active.load(Ordering::SeqCst),
            "请等待环境操作完成"
        );
        ensure!(
            !self
                .db()
                .list_jobs()?
                .iter()
                .any(|job| job.asset_id == asset_id
                    && ["queued", "running"].contains(&job.status.as_str())),
            "该课程已有进行中的任务"
        );
        let mut job = Job {
            id: new_id(),
            asset_id: asset_id.into(),
            title: asset.title,
            status: "queued".into(),
            stage: "等待获取回听音频".into(),
            progress: 0.0,
            error: None,
            model: settings.model.clone(),
            device: settings.device.clone(),
            mode: "auto".into(),
            preset: settings.preset.clone(),
            chunk_done: 0,
            chunk_total: 0,
            created_at: now(),
            updated_at: now(),
        };
        self.save_snapshot(
            &job.id,
            &JobSnapshot {
                settings,
                part,
                audio_only: true,
                fallback_used: false,
            },
        )?;
        self.publish(&mut job)?;
        drop(_gate);
        self.kick();
        Ok(job)
    }
    pub fn asset_detail(&self, id: &str) -> Result<AssetDetail> {
        let db = self.db();
        let mut asset = db.get_asset(id)?;
        // The asset protocol rejects parent components, including in older cache paths.
        // Expose the same resolved file to both the protocol scope and the player.
        asset.audio_path = asset
            .audio_path
            .as_deref()
            .and_then(|path| dunce::canonicalize(path).ok())
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned());
        Ok(AssetDetail {
            asset,
            transcript: db.active_transcript(id)?,
            versions: db.list_transcripts(id)?,
            notes: db.list_notes(id)?,
        })
    }
    pub fn check_integrity(
        &self,
        asset_id: &str,
        transcript_id: &str,
    ) -> Result<super::integrity::IntegrityReport> {
        let db = self.db();
        let asset = db.get_asset(asset_id)?;
        let t = db.get_transcript(transcript_id)?;
        ensure!(t.asset_id == asset_id, "文字版本不属于当前课程");
        let job = db.transcript_job(transcript_id)?;
        let source_duration = job
            .as_ref()
            .and_then(|j| self.read_snapshot(&j.id).ok())
            .map(|s| s.part.duration_ms)
            .filter(|d| *d > 0);
        let mut report = super::integrity::check(&asset, &t, job.as_ref(), source_duration);
        report.review = db
            .integrity_review(transcript_id, &report.fingerprint)?
            .map(|(note, reviewed_at)| super::integrity::Review { note, reviewed_at });
        Ok(report)
    }
    pub fn review_integrity(
        &self,
        asset_id: &str,
        transcript_id: &str,
        fingerprint: &str,
        note: &str,
    ) -> Result<super::integrity::IntegrityReport> {
        let _gate = self.mutation_gate.lock().unwrap();
        let report = self.check_integrity(asset_id, transcript_id)?;
        ensure!(
            report.fingerprint == fingerprint,
            "检查依据已变化，请刷新报告后重新核对"
        );
        self.db()
            .save_integrity_review(transcript_id, fingerprint, note)?;
        self.check_integrity(asset_id, transcript_id)
    }
    pub fn initialize_vault(&self) -> Result<String> {
        let _gate = self.mutation_gate.lock().unwrap();
        super::vault::initialize(Path::new(&self.settings().obsidian_vault))
    }
    pub fn sync_vault(
        &self,
        asset_id: &str,
        transcript_id: &str,
    ) -> Result<super::vault::SyncResult> {
        let _gate = self.mutation_gate.lock().unwrap();
        let db = self.db();
        let asset = db.get_asset(asset_id)?;
        let t = db.get_transcript(transcript_id)?;
        let report = self.check_integrity(asset_id, transcript_id)?;
        let notes: Vec<_> = db
            .list_notes(asset_id)?
            .into_iter()
            .filter(|n| n.transcript_id == transcript_id)
            .collect();
        super::vault::sync(
            Path::new(&self.settings().obsidian_vault),
            &asset,
            &t,
            &notes,
            &report,
        )
    }
    pub fn save_edit(
        &self,
        asset_id: &str,
        base_id: &str,
        segments: &[Segment],
    ) -> Result<Transcript> {
        let _gate = self.mutation_gate.lock().unwrap();
        let db = self.db();
        let active = db
            .active_transcript(asset_id)?
            .context("课程还没有文字稿")?;
        ensure!(
            active.id == base_id,
            "当前文字版本已改变，请重新加载后再保存，以免覆盖其他修订"
        );
        let existing: HashSet<_> = active.segments.iter().map(|s| s.id.as_str()).collect();
        ensure!(
            segments.len() == active.segments.len()
                && segments.iter().all(|s| existing.contains(s.id.as_str())),
            "文字校对必须保留原始片段 ID"
        );
        db.save_transcript(
            asset_id,
            "edited",
            active.model.as_deref(),
            &active.language,
            segments,
        )
    }
    pub fn activate_version(&self, asset_id: &str, transcript_id: &str) -> Result<()> {
        let _gate = self.mutation_gate.lock().unwrap();
        self.db().activate_transcript(asset_id, transcript_id)
    }
    pub fn search(&self, query: &str, asset_id: Option<&str>) -> Result<Vec<SearchHit>> {
        ensure!(query.chars().count() <= 300, "搜索词过长");
        self.db().search(query, asset_id)
    }
    pub fn export(
        &self,
        asset_id: &str,
        format: &str,
        destination: &str,
        transcript_id: Option<&str>,
    ) -> Result<String> {
        let db = self.db();
        let asset = db.get_asset(asset_id)?;
        let transcript = if let Some(id) = transcript_id {
            db.get_transcript(id)?
        } else {
            db.active_transcript(asset_id)?
                .context("课程没有可导出的文字稿")?
        };
        ensure!(
            transcript.asset_id == asset_id,
            "所选文字版本不属于当前课程"
        );
        let mut content = course_core::export::export_transcript(
            &transcript,
            &asset.title,
            matches!(asset.source_kind.as_str(), "bilibili" | "webMedia")
                .then_some(asset.source.as_str()),
            format,
        )?;
        if format == "md" {
            for note in db
                .list_notes(asset_id)?
                .into_iter()
                .filter(|note| note.transcript_id == transcript.id)
            {
                content.push_str(&format!("\n\n## {}\n\n{}\n", note.title, note.content));
                for cite in note.citations {
                    let link = if asset.bvid.is_some() {
                        format!("{}&t={}", asset.source, cite.start_ms / 1000)
                    } else {
                        format!("#segment-{}", cite.segment_id)
                    };
                    content.push_str(&format!(
                        "\n- [{}]({}): {}\n",
                        cite.segment_id, link, cite.text
                    ));
                }
            }
        }
        let destination = Path::new(destination);
        ensure!(destination.is_absolute(), "请选择绝对导出路径");
        ensure!(
            destination
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(
                    |ext| ["txt", "md", "srt", "vtt"].contains(&ext.to_ascii_lowercase().as_str())
                ),
            "导出文件扩展名应为 txt、md、srt 或 vtt"
        );
        settings::atomic_write(destination, content.as_bytes())?;
        Ok(destination.to_string_lossy().into_owned())
    }
    fn selected(transcript: &Transcript, ids: &[String]) -> Result<Vec<Segment>> {
        let wanted: HashSet<_> = ids.iter().map(String::as_str).collect();
        ensure!(wanted.len() == ids.len(), "片段选择存在重复");
        let selected: Vec<_> = transcript
            .segments
            .iter()
            .filter(|s| wanted.contains(s.id.as_str()))
            .cloned()
            .collect();
        ensure!(
            selected.len() == wanted.len(),
            "片段选择包含失效引用，请重新加载文字稿"
        );
        Ok(selected)
    }
    pub fn generate_knowledge(
        &self,
        asset_id: &str,
        transcript_id: &str,
        kind: &str,
        question: Option<&str>,
        segment_ids: &[String],
    ) -> Result<Note> {
        let (db, settings) = {
            let _gate = self.mutation_gate.lock().unwrap();
            (self.db(), self.settings())
        };
        let transcript = db.get_transcript(transcript_id)?;
        ensure!(transcript.asset_id == asset_id, "文字版本不属于当前课程");
        let segments = Self::selected(&transcript, segment_ids)?;
        let key = self.api_key()?;
        let (content, citations) =
            OpenAiCompatible.generate(&settings, key.as_deref(), kind, question, &segments)?;
        let note = Note {
            id: new_id(),
            asset_id: asset_id.into(),
            transcript_id: transcript.id,
            kind: kind.into(),
            title: if kind == "answer" {
                question.unwrap_or("课程问答").chars().take(80).collect()
            } else {
                "课程学习笔记".into()
            },
            content,
            citations,
            question: question.map(str::to_owned),
            created_at: now(),
            stale: false,
        };
        db.save_note(&note)?;
        Ok(note)
    }
    pub fn save_manual_note(
        &self,
        asset_id: &str,
        transcript_id: &str,
        title: &str,
        content: &str,
        ids: &[String],
    ) -> Result<Note> {
        ensure!(
            !content.trim().is_empty() && content.chars().count() <= 100000,
            "笔记不能为空且最多 10 万字"
        );
        let db = self.db();
        let transcript = db.get_transcript(transcript_id)?;
        ensure!(transcript.asset_id == asset_id, "文字版本不属于当前课程");
        let citations = Self::selected(&transcript, ids)?
            .into_iter()
            .map(|s| Citation {
                segment_id: s.id,
                start_ms: s.start_ms,
                end_ms: s.end_ms,
                text: s.text,
            })
            .collect();
        let note = Note {
            id: new_id(),
            asset_id: asset_id.into(),
            transcript_id: transcript_id.into(),
            kind: "manual".into(),
            title: if title.trim().is_empty() {
                "学习笔记".into()
            } else {
                title.chars().take(200).collect()
            },
            content: content.into(),
            citations,
            question: None,
            created_at: now(),
            stale: false,
        };
        db.save_note(&note)?;
        Ok(note)
    }
    fn credential(&self) -> Result<keyring::Entry> {
        keyring::Entry::new("local.courseworkbench.desktop", "knowledge-api-key")
            .context("无法访问系统凭据存储")
    }
    fn api_key(&self) -> Result<Option<String>> {
        match self.credential()?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(error).context("无法读取系统凭据，文字稿仍保存在本地"),
        }
    }
    pub fn set_api_key(&self, key: &str) -> Result<bool> {
        ensure!(
            !key.contains(['\r', '\n']) && key.len() < 16000,
            "密钥格式不正确"
        );
        let entry = self.credential()?;
        if key.trim().is_empty() {
            match entry.delete_credential() {
                Ok(()) | Err(keyring::Error::NoEntry) => Ok(false),
                Err(error) => Err(error.into()),
            }
        } else {
            entry.set_password(key.trim())?;
            Ok(true)
        }
    }
    fn begin_auxiliary(&self) -> Result<AuxiliaryGuard<'_>> {
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(!self.shutting_down.load(Ordering::SeqCst), "应用正在退出");
        ensure!(
            !self.worker_active.load(Ordering::SeqCst)
                && !self
                    .db()
                    .list_jobs()?
                    .iter()
                    .any(|job| ["running", "queued"].contains(&job.status.as_str())),
            "请先完成或取消正在处理的任务"
        );
        ensure!(
            self.auxiliary_active
                .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok(),
            "另一项环境操作正在运行"
        );
        let id = format!("aux-{}", new_id());
        let control = Arc::new(ProcessControl::default());
        self.controls
            .lock()
            .unwrap()
            .insert(id.clone(), control.clone());
        Ok(AuxiliaryGuard {
            runtime: self,
            id,
            control,
            exclusive: true,
        })
    }
    pub fn save_settings(&self, new_settings: AppSettings) -> Result<AppSettings> {
        let _guard = self.begin_auxiliary()?;
        let _gate = self.mutation_gate.lock().unwrap();
        new_settings.validate()?;
        new_settings.prepare_directories()?;
        let new_lock = if new_settings.data_path().canonicalize()?
            != self.settings().data_path().canonicalize()?
        {
            Some(lock_library(&new_settings)?)
        } else {
            None
        };
        let db = Db::open(&new_settings.data_path().join("library.sqlite3"))?;
        db.recover_jobs()?;
        settings::save(&self.config_path, &new_settings)?;
        *self.database.write().unwrap() = db;
        *self.settings.write().unwrap() = new_settings.clone();
        if let Some(lock) = new_lock {
            *self.library_lock.lock().unwrap() = lock;
        }
        self.config_revision.fetch_add(1, Ordering::SeqCst);
        *self.report.lock().unwrap() = None;
        Ok(new_settings)
    }
    pub fn cache_inventory(&self) -> Result<Vec<cache::CacheCategory>> {
        cache::inventory(&self.settings())
    }
    pub fn clear_cache(&self, category: &str) -> Result<()> {
        let _guard = self.begin_auxiliary()?;
        let settings = self.settings();
        cache::clear(&settings, category)?;
        if category == "audio" {
            let db = self.db();
            for mut asset in db.list_assets()? {
                if asset
                    .audio_path
                    .as_ref()
                    .is_some_and(|path| !Path::new(path).is_file())
                {
                    asset.audio_path = None;
                    db.upsert_asset(&asset)?;
                }
            }
        }
        Ok(())
    }
    pub fn download_model(&self, model: &str) -> Result<Value> {
        let _guard = self.begin_auxiliary()?;
        let settings = self.settings();
        let worker = WhisperWorker {
            path: self.worker_path.clone(),
        };
        let result=worker.execute(&settings,json!({"command":"download_model","job_id":"model-download","model":model,"allow_download":true}),&_guard.control,&mut |_|Ok(()))?;
        if Path::new(&settings.model_dir).canonicalize().ok()
            == settings.data_path().join("models").canonicalize().ok()
        {
            fs::write(
                Path::new(&settings.model_dir).join(".course-workbench-owned"),
                b"Course Workbench managed model cache\n",
            )?;
        }
        *self.report.lock().unwrap() = None;
        Ok(result)
    }
    pub fn benchmark_profile(
        &self,
        asset_id: &str,
        model: &str,
        device: &str,
    ) -> Result<BenchmarkRecord> {
        let _guard = self.begin_auxiliary()?;
        ensure!(["cpu", "cuda"].contains(&device), "请选择 CPU 或 CUDA");
        let settings = self.settings();
        let asset = self.db().get_asset(asset_id)?;
        let control = &_guard.control;
        let audio = media::ensure_audio(&settings, &asset, control, |_, _| Ok(()))?;
        let id = new_id();
        let sample = settings
            .cache_path("temp")
            .join(format!("benchmark-{id}.wav"));
        media::convert(&settings, &audio, &sample, control, Some(60), |_| Ok(()))?;
        let report = self.probe_resources()?;
        let request = transcribe_request(
            &settings,
            &id,
            &sample,
            model,
            device,
            &settings.cache_path("temp").join(format!("benchmark-{id}")),
        );
        let worker = WhisperWorker {
            path: self.worker_path.clone(),
        };
        let result = worker.execute(&settings, request, control, &mut |_| Ok(()));
        let record = match result {
            Ok(value) => benchmark::from_result(
                &settings,
                &report,
                asset_id,
                model,
                device,
                asset.duration_ms.min(60000) as f64 / 1000.0,
                &value,
            ),
            Err(error) => BenchmarkRecord {
                id,
                asset_id: asset_id.into(),
                model: model.into(),
                device: device.into(),
                threads: settings.threads,
                gpu_concurrency: 1,
                audio_seconds: asset.duration_ms.min(60000) as f64 / 1000.0,
                elapsed_seconds: 0.0,
                peak_ram_mb: None,
                peak_gpu_mb: None,
                peak_gpu_reserved_mb: None,
                environment_fingerprint: benchmark::fingerprint(&settings, &report),
                created_at: now(),
                success: false,
                error: Some(format!("{error:#}")),
                tested: true,
            },
        };
        benchmark::save(&settings, &record)?;
        Ok(record)
    }
    pub fn list_benchmarks(&self) -> Result<Vec<BenchmarkRecord>> {
        benchmark::list(&self.settings())
    }
    pub fn shutdown(&self) {
        self.shutting_down.store(true, Ordering::SeqCst);
        for control in self.controls.lock().unwrap().values() {
            control.cancel();
        }
    }
}
