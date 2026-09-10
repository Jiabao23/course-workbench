use super::{
    benchmark::BenchmarkRecord,
    cache::CacheCategory,
    profiler::ResourceReport,
    service::{AssetDetail, Bootstrap, CreateJobsRequest, Runtime},
    settings::AppSettings,
    source::{SourcePart, SourcePreview},
};
use course_core::{Job, Note, SearchHit, Segment, Transcript};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

type Reply<T> = Result<T, String>;
#[tauri::command]
pub async fn check_integrity(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
) -> Reply<super::integrity::IntegrityReport> {
    blocking(state.inner().clone(), move |s| {
        s.check_integrity(&asset_id, &transcript_id)
    })
    .await
}
#[tauri::command]
pub async fn review_integrity(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
    fingerprint: String,
    note: String,
) -> Reply<super::integrity::IntegrityReport> {
    blocking(state.inner().clone(), move |s| {
        s.review_integrity(&asset_id, &transcript_id, &fingerprint, &note)
    })
    .await
}
#[tauri::command]
pub async fn initialize_vault(state: State<'_, Arc<Runtime>>) -> Reply<String> {
    blocking(state.inner().clone(), move |s| s.initialize_vault()).await
}
#[tauri::command]
pub async fn sync_vault(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
) -> Reply<super::vault::SyncResult> {
    blocking(state.inner().clone(), move |s| {
        s.sync_vault(&asset_id, &transcript_id)
    })
    .await
}
#[tauri::command]
pub fn open_vault_note(
    app: AppHandle,
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
) -> Reply<()> {
    let path = super::vault::index_path(
        std::path::Path::new(&state.settings().obsidian_vault),
        &asset_id,
    )
    .map_err(|e| e.to_string())?;
    let uri = super::vault::open_uri(&path).map_err(|e| e.to_string())?;
    app.opener().open_url(uri,None::<&str>).map_err(|e|format!("无法打开 Obsidian：{e}。请安装并启动 Obsidian，在库管理器中把设置中的知识库文件夹作为仓库打开。Markdown 文件已保存在本地。"))
}
async fn blocking<T: Send + 'static>(
    state: Arc<Runtime>,
    work: impl FnOnce(Arc<Runtime>) -> anyhow::Result<T> + Send + 'static,
) -> Reply<T> {
    tauri::async_runtime::spawn_blocking(move || work(state).map_err(|error| format!("{error:#}")))
        .await
        .map_err(|error| error.to_string())?
}
#[tauri::command]
pub async fn bootstrap(state: State<'_, Arc<Runtime>>) -> Reply<Bootstrap> {
    blocking(state.inner().clone(), |state| state.bootstrap()).await
}
#[tauri::command]
pub async fn probe_source(state: State<'_, Arc<Runtime>>, source: String) -> Reply<SourcePreview> {
    blocking(state.inner().clone(), move |state| {
        state.probe_source(&source)
    })
    .await
}
#[tauri::command]
pub async fn probe_part(
    state: State<'_, Arc<Runtime>>,
    source: String,
    page: u32,
) -> Reply<SourcePart> {
    blocking(state.inner().clone(), move |state| {
        state.probe_part(&source, page)
    })
    .await
}
#[tauri::command]
pub async fn create_jobs(
    state: State<'_, Arc<Runtime>>,
    request: CreateJobsRequest,
) -> Reply<Vec<Job>> {
    blocking(state.inner().clone(), move |state| {
        state.create_jobs(request)
    })
    .await
}
#[tauri::command]
pub async fn cancel_job(state: State<'_, Arc<Runtime>>, job_id: String) -> Reply<Job> {
    blocking(state.inner().clone(), move |state| {
        state.cancel_job(&job_id)
    })
    .await
}
#[tauri::command]
pub async fn retry_job(
    state: State<'_, Arc<Runtime>>,
    job_id: String,
    use_current_settings: bool,
) -> Reply<Job> {
    blocking(state.inner().clone(), move |state| {
        state.retry_job(&job_id, use_current_settings)
    })
    .await
}
#[tauri::command]
pub async fn get_asset_detail(
    app: AppHandle,
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
) -> Reply<AssetDetail> {
    let detail = blocking(state.inner().clone(), move |state| {
        state.asset_detail(&asset_id)
    })
    .await?;
    if let Some(path) = detail
        .asset
        .audio_path
        .as_ref()
        .filter(|path| std::path::Path::new(path).is_file())
    {
        app.asset_protocol_scope()
            .allow_file(path)
            .map_err(|error| error.to_string())?;
    }
    Ok(detail)
}
#[tauri::command]
pub async fn save_transcript_edit(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    base_transcript_id: String,
    segments: Vec<Segment>,
) -> Reply<Transcript> {
    blocking(state.inner().clone(), move |state| {
        state.save_edit(&asset_id, &base_transcript_id, &segments)
    })
    .await
}
#[tauri::command]
pub async fn activate_version(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
) -> Reply<()> {
    blocking(state.inner().clone(), move |state| {
        state.activate_version(&asset_id, &transcript_id)
    })
    .await
}
#[tauri::command]
pub async fn ensure_audio(state: State<'_, Arc<Runtime>>, asset_id: String) -> Reply<Job> {
    blocking(state.inner().clone(), move |state| {
        state.ensure_audio(&asset_id)
    })
    .await
}
#[tauri::command]
pub async fn export_asset(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    format: String,
    destination: String,
    transcript_id: Option<String>,
) -> Reply<String> {
    blocking(state.inner().clone(), move |state| {
        state.export(&asset_id, &format, &destination, transcript_id.as_deref())
    })
    .await
}
#[tauri::command]
pub async fn search_library(
    state: State<'_, Arc<Runtime>>,
    query: String,
    asset_id: Option<String>,
) -> Reply<Vec<SearchHit>> {
    blocking(state.inner().clone(), move |state| {
        state.search(&query, asset_id.as_deref())
    })
    .await
}
#[tauri::command]
pub async fn generate_knowledge(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
    kind: String,
    question: Option<String>,
    segment_ids: Vec<String>,
) -> Reply<Note> {
    blocking(state.inner().clone(), move |state| {
        state.generate_knowledge(
            &asset_id,
            &transcript_id,
            &kind,
            question.as_deref(),
            &segment_ids,
        )
    })
    .await
}
#[tauri::command]
pub async fn save_manual_note(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    transcript_id: String,
    title: String,
    content: String,
    segment_ids: Vec<String>,
) -> Reply<Note> {
    blocking(state.inner().clone(), move |state| {
        state.save_manual_note(&asset_id, &transcript_id, &title, &content, &segment_ids)
    })
    .await
}
#[tauri::command]
pub async fn save_settings(
    state: State<'_, Arc<Runtime>>,
    settings: AppSettings,
) -> Reply<AppSettings> {
    blocking(state.inner().clone(), move |state| {
        state.save_settings(settings)
    })
    .await
}
#[tauri::command]
pub async fn set_api_key(state: State<'_, Arc<Runtime>>, api_key: String) -> Reply<bool> {
    blocking(state.inner().clone(), move |state| {
        state.set_api_key(&api_key)
    })
    .await
}
#[tauri::command]
pub async fn probe_resources(state: State<'_, Arc<Runtime>>) -> Reply<ResourceReport> {
    blocking(state.inner().clone(), |state| state.probe_resources()).await
}
#[tauri::command]
pub async fn benchmark_profile(
    state: State<'_, Arc<Runtime>>,
    asset_id: String,
    model: String,
    device: String,
) -> Reply<BenchmarkRecord> {
    blocking(state.inner().clone(), move |state| {
        state.benchmark_profile(&asset_id, &model, &device)
    })
    .await
}
#[tauri::command]
pub async fn list_benchmarks(state: State<'_, Arc<Runtime>>) -> Reply<Vec<BenchmarkRecord>> {
    blocking(state.inner().clone(), |state| state.list_benchmarks()).await
}
#[tauri::command]
pub async fn download_model(state: State<'_, Arc<Runtime>>, model: String) -> Reply<Value> {
    blocking(state.inner().clone(), move |state| {
        state.download_model(&model)
    })
    .await
}
#[tauri::command]
pub async fn cache_inventory(state: State<'_, Arc<Runtime>>) -> Reply<Vec<CacheCategory>> {
    blocking(state.inner().clone(), |state| state.cache_inventory()).await
}
#[tauri::command]
pub async fn clear_cache_category(state: State<'_, Arc<Runtime>>, category: String) -> Reply<()> {
    blocking(state.inner().clone(), move |state| {
        state.clear_cache(&category)
    })
    .await
}
#[tauri::command]
pub fn open_external(app: AppHandle, target: String) -> Reply<()> {
    let url = super::settings::safe_web_url(&target).map_err(|error| error.to_string())?;
    app.opener()
        .open_url(url.as_str(), None::<&str>)
        .map_err(|error| error.to_string())
}
#[tauri::command]
pub fn open_data_folder(app: AppHandle, state: State<'_, Arc<Runtime>>) -> Reply<()> {
    app.opener()
        .open_path(state.settings().data_dir, None::<&str>)
        .map_err(|error| error.to_string())
}
