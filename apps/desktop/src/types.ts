export interface Segment { id:string; startMs:number; endMs:number; text:string }
export interface Asset { id:string; title:string; sourceKind:string; source:string; bvid:string|null; page:number|null; durationMs:number; audioPath:string|null; activeVersionId:string|null; createdAt:string; updatedAt:string }
export interface Transcript { id:string; assetId:string; version:number; sourceKind:string; model:string|null; language:string; segments:Segment[]; createdAt:string; isActive:boolean }
export interface Job { id:string; assetId:string; title:string; status:'queued'|'running'|'paused'|'completed'|'failed'|'cancelled'; stage:string; progress:number; error:string|null; model:string; device:string; mode:string; preset:string; chunkDone:number; chunkTotal:number; createdAt:string; updatedAt:string }
export interface Citation { segmentId:string; startMs:number; endMs:number; text:string }
export interface Note { id:string; assetId:string; transcriptId:string; kind:'summary'|'answer'|'manual'; title:string; content:string; citations:Citation[]; question:string|null; createdAt:string; stale:boolean }
export interface SearchHit { assetId:string; assetTitle:string; transcriptId:string; segmentId:string; startMs:number; endMs:number; text:string; score:number }
export interface GpuInfo { name:string; totalMb:number; freeMb:number; driver:string }
export interface SystemResources { cpuName:string; logicalCores:number; ramTotalMb:number; ramAvailableMb:number; diskFreeMb:number; gpu:GpuInfo|null; cudaAvailable:boolean; pythonAvailable:boolean; torchVersion:string|null; warnings:string[] }
export interface ResourceRecommendation { model:string; device:string; threads:number; gpuConcurrency:number; maxGpuConcurrency:number; reason:string; warnings:string[] }
export interface AppSettings { dataDir:string; modelDir:string; pythonPath:string; qualityPackagesDir:string; ffmpegPath:string; ffprobePath:string; ytDlpPath:string; preset:'eco'|'balanced'|'quality'|'custom'; model:string; device:'auto'|'cpu'|'cuda'; threads:number; gpuConcurrency:number; language:string; prompt:string; llmBaseUrl:string; llmModel:string; llmContextChars:number; cookieFile:string; setupComplete:boolean; obsidianVault:string; theme:Theme }
export interface Bootstrap { organization:Organization; assets:Asset[]; jobs:Job[]; settings:AppSettings; resources:SystemResources; recommendation:ResourceRecommendation; apiKeyConfigured:boolean }
export interface AssetDetail { asset:Asset; transcript:Transcript|null; versions:Transcript[]; notes:Note[] }
export type SubtitleStatus='available'|'absent'|'loginRequired'|'failed'|'unchecked';
export interface SubtitleTrack { language:string; label:string; url:string; format:string; automatic:boolean }
export interface SourcePart { page:number; cid:number|null; title:string; durationMs:number; subtitleStatus:SubtitleStatus; subtitles:SubtitleTrack[] }
export interface SourcePreview { source:string; title:string; sourceKind:string; bvid:string|null; parts:SourcePart[]; warnings:string[] }
export interface ResourceReport { resources:SystemResources; recommendation:ResourceRecommendation; models:{name:string;size_bytes:number}[]; dependencies:Record<string,boolean> }
export interface BenchmarkRecord { id:string; assetId:string; model:string; device:string; threads:number; gpuConcurrency:number; audioSeconds:number; elapsedSeconds:number; peakRamMb:number|null; peakGpuMb:number|null; peakGpuReservedMb:number|null; environmentFingerprint:string; createdAt:string; success:boolean; error:string|null; tested:boolean }
export interface CacheCategory { id:string; title:string; path:string; sizeBytes:number; fileCount:number; clearable:boolean; warning:string|null }

export interface IntegrityIssue { id:string; code:string; severity:'warning'|'info'; startMs:number; endMs:number; message:string; resolution:{status:'confirmed'|'pending'|'revised';note:string;reviewedAt:string}|null }
export interface IntegrityReport { transcriptId:string; version:number; status:string; durationMs:number|null; coveredMs:number; segmentCount:number; chunkDone:number|null; chunkTotal:number|null; issues:IntegrityIssue[]; limitations:string; fingerprint:string; review:{note:string;reviewedAt:string}|null; historicalReview?:{note:string;reviewedAt:string}|null; pendingCount:number; diagnosticsAvailable:boolean; audioCheck:{status:'notRun'|'available'|'unavailable';message:string} }
export interface RecheckCandidate { id:string;assetId:string;transcriptId:string;startMs:number;endMs:number;model:string;device:string;originalText:string;segments:Segment[];createdAt:string }
export interface VaultSyncResult { snapshotPath:string; indexPath:string; personalPath:string; snapshotLink:string; openUri:string; created:boolean }

export type Theme='forest'|'paper'|'night';
export interface Collection {id:string;name:string;parentId:string|null}
export interface AssetOrganization {assetId:string;collectionId:string|null;favorite:boolean}
export interface Organization {collections:Collection[];entries:AssetOrganization[]}
