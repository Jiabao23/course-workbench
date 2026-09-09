# Shared implementation contract, v1

The Rust core is independent of Tauri. All records below serialize in camelCase.
Use String for identifiers, UTC RFC3339 strings for dates, u64 for milliseconds
and MB quantities, f64 for percentage and timing. Optional fields serialize null.

## Core records

- Segment: id, startMs, endMs, text.
- Asset: id, title, sourceKind, source, bvid?, page? (u32), durationMs,
  audioPath?, activeVersionId?, createdAt, updatedAt.
- Transcript: id, assetId, version (u32), sourceKind, model?, language,
  segments (Segment[]), createdAt, isActive (bool).
- Job: id, assetId, title, status, stage, progress (0..100), error?, model,
  device, mode, preset, chunkDone (u32), chunkTotal (u32), createdAt, updatedAt.
  status: queued/running/paused/completed/failed/cancelled.
  mode: auto/subtitlesOnly/transcribe; preset: eco/balanced/quality/custom.
- Citation: segmentId, startMs, endMs, text.
- Note: id, assetId, transcriptId, kind (summary/answer/manual), title,
  content (Markdown), citations (Citation[]), question?, createdAt, stale (bool).
- SearchHit: assetId, assetTitle, transcriptId, segmentId, startMs, endMs,
  text, score (f64).
- GpuInfo: name, totalMb, freeMb, driver.
- SystemResources: cpuName, logicalCores (u32), ramTotalMb, ramAvailableMb,
  diskFreeMb, gpu? (GpuInfo), cudaAvailable (bool), pythonAvailable (bool),
  torchVersion?, warnings (String[]).
- ResourceRecommendation: model, device, threads (u32), gpuConcurrency (u32),
  maxGpuConcurrency (u32), reason, warnings (String[]).

## Core module ownership and API

Core agent owns crates/core/src/{lib,types,db,subtitles,export,resources,knowledge}.rs.
It may add focused tests and helpers inside that crate. Root owns service.rs and
source.rs added later; do not declare those modules until root integration.

`Db` holds a PathBuf (open connection per call, WAL + busy timeout); it must be
Send + Sync. `Db::open(&Path) -> Result<Db>` ensures schema. Required methods:
upsert_asset(&Asset), list_assets()->Vec<Asset>, get_asset(id)->Asset,
save_transcript(asset_id,source_kind,model:Option<&str>,language,&[Segment])->Transcript,
get_transcript(id)->Transcript, active_transcript(asset_id)->Option<Transcript>,
list_transcripts(asset_id)->Vec<Transcript>, activate_transcript(asset_id,id),
upsert_job(&Job), get_job(id)->Job, list_jobs()->Vec<Job>,
recover_jobs()->usize (running/queued -> paused, retain checkpoint),
save_note(&Note), list_notes(asset_id)->Vec<Note>,
search(query, asset_id:Option<&str>)->Vec<SearchHit>.
All methods return anyhow::Result except path accessor. All mutations take &self.
Transcript save is transactional: immutable version, update active id, rebuild
only that asset's active FTS index. Chinese query text tokenized with jieba-rs.
Note.stale is computed by comparing bound version with asset.activeVersionId.

`parse_subtitles(text:&str,format:&str)->Result<Vec<Segment>>` handles SRT,VTT,
Bilibili JSON body[{from,to,content}], validates ordered nonnegative timing,
multiline captions; preserve original strings safely. Stable IDs for parsed
segments; editing must retain IDs. Reject empty/non-caption input.
`export_transcript(&Transcript,title:&str,source_url:Option<&str>,format:&str)
 -> Result<String>` for txt/md/srt/vtt, exact millisecond timestamps.
`recommend(&SystemResources,preset:&str)->ResourceRecommendation` is a pure
policy function: CPU fallback if CUDA isn't actually available; GPU concurrency
defaults 1, never approve larger concurrency solely from total VRAM; retain
headroom based on free memory; candidates are not validated benchmarks.
`validate_knowledge_response(raw:&str,segments:&[Segment])
 -> Result<(String,Vec<Citation>)>` parses JSON {content,citations:[segmentId]},
allows fenced JSON, rejects unknown citations, requires evidence for content
except explicit JSON insufficientEvidence:true (return an insufficiency message).
Avoid regex treating subtitle/LLM text as executable code or HTML.

## Python ASR worker protocol

Spawn selected Python with `worker.py`. Read one JSON line request from stdin:
{"protocol_version":1,"command":"probe"|"transcribe"|"download_model",
 "job_id":"...","audio_path":"...","model":"small","device":"cuda",
 "model_dir":"...","checkpoint_dir":"...","threads":4,"language":"zh",
 "prompt":"...","chunk_seconds":300,"allow_download":false}.
stdout: JSON lines with protocol_version=1, job_id, type=
hello/progress/segment/checkpoint/done/error. Logs go only to stderr.
probe done includes python_version,torch_version,cuda_version,cuda_available,
gpu_name,gpu_total_mb,gpu_free_mb,models and dependency status.
transcribe progress carries progress (0..100),chunk_done,chunk_total;
segment carries {id,start_ms,end_ms,text}; done carries language,model,device,
elapsed_seconds,peak_ram_mb,peak_gpu_mb,segments, resumed_chunks.
error carries code,message,retryable. OOM => code=out_of_memory.
Checkpoints are keyed by audio content hash + model/device/language/prompt/chunk
settings; completed chunks committed atomically; cancellation uses process tree
termination. Never mix checkpoint output from different models/settings.

## Desktop integration

Root supplies commands: bootstrap, probe_source, create_jobs, cancel_job,
retry_job, get_asset_detail, save_transcript_edit, activate_version,
export_asset, search_library, generate_knowledge, save_manual_note,
save_settings, set_api_key, probe_resources, benchmark_profile,
download_model, cache_inventory, clear_cache_category, open_external.
Single `job-updated` event payload is a Job record. Refetch authoritative data.
Core state owns database; frontend cannot mutate raw paths or issue shell code.

Bootstrap -> {assets,jobs,settings,resources,recommendation,apiKeyConfigured}.
Source preview -> {source,title,sourceKind,bvid,parts:[{page,cid,title,durationMs,
subtitleStatus,subtitles:[{language,label,url,format}]}],warnings}.
Asset detail -> {asset,transcript:Transcript|null,versions:Transcript[],notes:Note[]}.
AppSettings -> {dataDir,modelDir,pythonPath,ffmpegPath,ffprobePath,ytDlpPath,
preset,model,device,threads,gpuConcurrency,language,prompt,
llmBaseUrl,llmModel,llmContextChars,cookieFile,setupComplete} (no API key).
