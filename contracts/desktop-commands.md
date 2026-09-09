# Desktop commands v1

All Tauri arguments and results use camelCase. Invoke commands via the typed
frontend API. Long operations run outside the window thread. `job-updated`
events carry the persisted Job; refresh data on completion. No frontend mocks
in production; a browser preview shows a truthful "open the desktop app" state.

| Command | Arguments | Result |
| --- | --- | --- |
| bootstrap | none | `{assets,jobs,settings,resources,recommendation,apiKeyConfigured}` |
| probe_source | `{source}` | SourcePreview |
| probe_part | `{source,page}` | SourcePart |
| create_jobs | `{request:{source,pages:number[],mode}}` | Job[] |
| cancel_job | `{jobId}` | Job |
| retry_job | `{jobId,useCurrentSettings:boolean}` | Job |
| get_asset_detail | `{assetId}` | `{asset,transcript,versions,notes}` |
| save_transcript_edit | `{assetId,baseTranscriptId,segments}` | Transcript (new version) |
| activate_version | `{assetId,transcriptId}` | void |
| ensure_audio | `{assetId}` | Job (audio only, transcript preserved) |
| export_asset | `{assetId,format,destination,transcriptId:string|null}` | destination path |
| search_library | `{query,assetId:string|null}` | SearchHit[] |
| generate_knowledge | `{assetId,transcriptId,kind,question:string|null,segmentIds:string[]}` | Note |
| save_manual_note | `{assetId,transcriptId,title,content,segmentIds:string[]}` | Note |
| save_settings | `{settings}` | AppSettings |
| set_api_key | `{apiKey}` | bool (empty input removes key) |
| probe_resources | none | `{resources,recommendation,models,dependencies}` |
| benchmark_profile | `{assetId,model,device}` | BenchmarkRecord |
| list_benchmarks | none | BenchmarkRecord[] |
| download_model | `{model}` | worker download result |
| cache_inventory | none | `{id,title,path,sizeBytes,fileCount,clearable,warning}[]` |
| clear_cache_category | `{category}` | void |
| open_external | `{target}` | void (HTTP/S only) |
| open_data_folder | none | void |

SourcePart subtitleStatus is `available`, `absent`, `loginRequired`, `failed` or
`unchecked`. Preview checks first part; the UI may check another selected part
via probe_part. Processing always checks the selected part again to refresh
expiring URLs. No part is implicitly submitted. Default selection is first only.

Job modes: auto, subtitlesOnly, transcribe. A retry uses the previous snapshot
unless useCurrentSettings=true; changed models have distinct checkpoint keys
and produce a new transcript version. GPU concurrency in v1 is fixed to one.

BenchmarkRecord: id, assetId, model, device, threads, gpuConcurrency,
audioSeconds, elapsedSeconds, peakRamMb, peakGpuMb, peakGpuReservedMb,
environmentFingerprint, createdAt, success, error, tested (always true for
records produced by actual worker runs). GPU peaks refer to PyTorch allocator.

Knowledge sends only explicitly selected segments. The UI displays selection
count and character count before a user clicks Generate. Empty selection or
an exceeded configured limit is an error, never silently widened/truncated.
No live provider is configured by default. OS credential vault stores the key.
