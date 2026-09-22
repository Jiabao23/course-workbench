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
subtitleStatus,subtitles:[{language,label,url,format,automatic}]}],warnings}.
Asset/preview sourceKind: bilibili/localMedia/subtitle/webMedia. Transcript
sourceKind additionally records bilibiliSubtitle/webSubtitle/whisper/edited.
Web sources have one part (page=1,cid=null,bvid=null); source is the original
HTTP(S) webpage or direct media URL. Duration 0 means unknown until probing.
SourceProvider::subtitles(source,track) receives the page URL for yt-dlp to
refresh time-limited caption URLs. Metadata and caption requests never download
media. Web caption formats are SRT/VTT; manual captions precede automatic
captions within a preferred language. The automatic flag defaults to false when
reading old job snapshots. Download flags request the chosen caption kind only,
so an unreadable manual track cannot shadow a readable automatic one of the
same language. Preview warnings disclose mixed-media
fallback and unknown duration. No database schema change is needed.
Native file selection and webview drag/drop accept exactly one supported local
file; stale preview responses must not overwrite a newer source selection.
Asset detail -> {asset,transcript:Transcript|null,versions:Transcript[],notes:Note[]}.
AppSettings -> {dataDir,modelDir,pythonPath,ffmpegPath,ffprobePath,ytDlpPath,
preset,model,device,threads,gpuConcurrency,language,prompt,
llmBaseUrl,llmModel,llmContextChars,cookieFile,obsidianVault,setupComplete} (no API key).

## Integrity and local vault (desktop v0.3.0)

SQLite schema 3 adds integrity_reviews keyed by transcript ID and evidence
fingerprint. Reviews never change immutable transcript text or processing evidence.
Back up schema 2 databases before upgrade; old binaries cannot open schema 3.

Commands: check_integrity(assetId,transcriptId),
review_integrity(assetId,transcriptId,fingerprint,note), initialize_vault(),
sync_vault(assetId,transcriptId), open_vault_note(assetId).
IntegrityReport -> {transcriptId,version,status,durationMs?,coveredMs,segmentCount,
chunkDone?,chunkTotal?,issues:[{code,startMs,endMs,message}],limitations,
fingerprint,review:{note,reviewedAt}|null}.
status is needsReview/noObviousIssues, never a claim of word accuracy.
Reports use union coverage, independent source duration and originating job
evidence. Standalone captions have unknown media duration. An edited version
does not borrow another version's job completion evidence. Review submission
rechecks its fingerprint; changed evidence requires a fresh review.

ASR transcripts are committed only after all expected 300-second chunks are
complete; one sub-millisecond remainder at a rounded chunk boundary is allowed.
Worker errors and missing chunks retain checkpoints and do not publish partial
transcripts as completed.

SyncResult -> {snapshotPath,indexPath,personalPath,snapshotLink,openUri,created}.
Vault sync writes only the configured local root's CourseWorkbench directory.
Snapshots are content-addressed Markdown with stable Obsidian block IDs and
version-bound notes/reviews. Identical sync is idempotent; modified snapshots
produce a conflict. Personal notes are never replaced; indexes append links
under a writer lock. Paths reject traversal and reparse points. Only internally
constructed Obsidian open URIs are passed to the system protocol handler.

## Organization and themes (desktop v0.4.0)

SQLite schema 4 adds collections and asset_organization. Back up schema 3 before
upgrading; downgrade requires restoring the matching pre-upgrade database.
Bootstrap adds organization: {collections: Collection[], entries: AssetOrganization[]}.
Collection -> {id,name,parentId}. AssetOrganization -> {assetId,collectionId,favorite}.
A missing entry is unclassified and not favorite. Collection depth includes the
root and is limited to 8; trimmed same-parent names compare by lowercase key.
Commands: create_collection(name,parentId), rename_collection(id,name),
delete_collection(id), move_assets(assetIds,collectionId), set_favorite(assetId,favorite).
Move validates all assets and destination atomically. Only empty collections are
deletable. Organization does not rename source/media/vault paths or versions.
AppSettings adds theme: forest|paper|night (default forest for older JSON).
Settings preview is temporary until save; discarding restores the saved theme.
Layout caches are local UI preferences; authority for collections/favorites is SQLite.

## Per-issue quality review (desktop v0.5.0)

SQLite schema 5 adds issue_reviews, quality_evidence, quality_evidence_history
and recheck_candidates. Back up schema 4 before upgrade. Reviews are keyed by
transcript, evidence fingerprint and issue ID; statuses are pending, confirmed
and revised. A revised record links to a new version and is not an accuracy claim.
Historical report-level notes remain visible but do not clear pending issues.

IntegrityReport adds pendingCount, diagnosticsAvailable, historicalReview and
audioCheck {status:notRun|available|unavailable,message}. Issues add stable id,
severity:warning|info and resolution:{status,note,reviewedAt}|null.
Audio checks bind full audio SHA256 and detector/model/runtime/options identity.
Replacing speech evidence archives the previous payload; diagnostic and
provenance evidence is immutable. Silence lowers a time-gap warning to info,
not deletion. Missing diagnostic values remain explicitly unavailable.

Commands: review_integrity_issue(assetId,transcriptId,fingerprint,issueId,status,note),
detect_speech(assetId,transcriptId), cancel_quality(),
recheck_interval(assetId,transcriptId,startMs,endMs),
list_recheck_candidates(assetId,transcriptId), adopt_candidate(candidateId),
discard_candidate(candidateId). Public review commands accept pending/confirmed;
revised is generated only by an explicit version-producing edit/adoption.

Candidates are persisted, limited to 120 seconds including whole segment
boundaries, and tied to the active source version and audio hash. Adoption is
transactional: new version, FTS, provenance, active pointer and related review
records; old notes and citations remain tied to their original version.
VAD runs on CPU; rechecks share the existing exclusive resource gate. Neither
command downloads audio or replaces text automatically.

AppSettings adds qualityPackagesDir (default empty). Only the separate VAD
worker receives that directory via PYTHONPATH. JSONL v1 transcribe segments may
include diagnostics {avg_logprob,no_speech_prob,compression_ratio}; older
checkpoints without diagnostics remain valid. quality_worker.py supports
detect_speech and lightweight detector_identity commands.
