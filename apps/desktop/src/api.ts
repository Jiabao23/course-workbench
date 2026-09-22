import { invoke, isTauri } from '@tauri-apps/api/core';
import type { Collection,AppSettings,AssetDetail,IntegrityReport,RecheckCandidate,VaultSyncResult,BenchmarkRecord,Bootstrap,CacheCategory,Job,Note,ResourceReport,SearchHit,Segment,SourcePart,SourcePreview,Transcript } from './types';

export const isDesktop = () => isTauri();
function call<T>(command:string,args?:Record<string,unknown>):Promise<T> {
  if (!isDesktop()) return Promise.reject(new Error('请在课程工作台桌面应用中使用此功能。'));
  return invoke<T>(command,args);
}
export const api = {
  createCollection:(name:string,parentId:string|null)=>call<Collection>('create_collection',{name,parentId}),
  renameCollection:(id:string,name:string)=>call<void>('rename_collection',{id,name}),
  deleteCollection:(id:string)=>call<void>('delete_collection',{id}),
  moveAssets:(assetIds:string[],collectionId:string|null)=>call<void>('move_assets',{assetIds,collectionId}),
  setFavorite:(assetId:string,favorite:boolean)=>call<void>('set_favorite',{assetId,favorite}),
  checkIntegrity:(assetId:string,transcriptId:string)=>call<IntegrityReport>('check_integrity',{assetId,transcriptId}),
  reviewIntegrityIssue:(assetId:string,transcriptId:string,fingerprint:string,issueId:string,status:'confirmed'|'pending',note:string)=>call<IntegrityReport>('review_integrity_issue',{assetId,transcriptId,fingerprint,issueId,status,note}),
  detectSpeech:(assetId:string,transcriptId:string)=>call<IntegrityReport>('detect_speech',{assetId,transcriptId}),
  cancelQuality:()=>call<void>('cancel_quality'),
  recheckInterval:(assetId:string,transcriptId:string,startMs:number,endMs:number)=>call<RecheckCandidate>('recheck_interval',{assetId,transcriptId,startMs,endMs}),
  listRecheckCandidates:(assetId:string,transcriptId:string)=>call<RecheckCandidate[]>('list_recheck_candidates',{assetId,transcriptId}),
  adoptCandidate:(candidateId:string)=>call<Transcript>('adopt_candidate',{candidateId}),
  discardCandidate:(candidateId:string)=>call<void>('discard_candidate',{candidateId}),
  reviewIntegrity:(assetId:string,transcriptId:string,fingerprint:string,note:string)=>call<IntegrityReport>('review_integrity',{assetId,transcriptId,fingerprint,note}),
  initializeVault:()=>call<string>('initialize_vault'),
  syncVault:(assetId:string,transcriptId:string)=>call<VaultSyncResult>('sync_vault',{assetId,transcriptId}),
  openVaultNote:(assetId:string)=>call<void>('open_vault_note',{assetId}),
  bootstrap:()=>call<Bootstrap>('bootstrap'),
  probeSource:(source:string)=>call<SourcePreview>('probe_source',{source}),
  probePart:(source:string,page:number)=>call<SourcePart>('probe_part',{source,page}),
  createJobs:(source:string,pages:number[],mode:string)=>call<Job[]>('create_jobs',{request:{source,pages,mode}}),
  cancelJob:(jobId:string)=>call<Job>('cancel_job',{jobId}),
  retryJob:(jobId:string,useCurrentSettings=false)=>call<Job>('retry_job',{jobId,useCurrentSettings}),
  assetDetail:(assetId:string)=>call<AssetDetail>('get_asset_detail',{assetId}),
  saveEdit:(assetId:string,baseTranscriptId:string,segments:Segment[])=>call<Transcript>('save_transcript_edit',{assetId,baseTranscriptId,segments}),
  activateVersion:(assetId:string,transcriptId:string)=>call<void>('activate_version',{assetId,transcriptId}),
  ensureAudio:(assetId:string)=>call<Job>('ensure_audio',{assetId}),
  exportAsset:(assetId:string,format:string,destination:string,transcriptId:string|null=null)=>call<string>('export_asset',{assetId,format,destination,transcriptId}),
  search:(query:string,assetId:string|null=null)=>call<SearchHit[]>('search_library',{query,assetId}),
  generateKnowledge:(assetId:string,transcriptId:string,kind:string,question:string|null,segmentIds:string[])=>call<Note>('generate_knowledge',{assetId,transcriptId,kind,question,segmentIds}),
  saveManualNote:(assetId:string,transcriptId:string,title:string,content:string,segmentIds:string[])=>call<Note>('save_manual_note',{assetId,transcriptId,title,content,segmentIds}),
  saveSettings:(settings:AppSettings)=>call<AppSettings>('save_settings',{settings}),
  setApiKey:(apiKey:string)=>call<boolean>('set_api_key',{apiKey}),
  probeResources:()=>call<ResourceReport>('probe_resources'),
  benchmarkProfile:(assetId:string,model:string,device:string)=>call<BenchmarkRecord>('benchmark_profile',{assetId,model,device}),
  listBenchmarks:()=>call<BenchmarkRecord[]>('list_benchmarks'),
  downloadModel:(model:string)=>call<Record<string,unknown>>('download_model',{model}),
  cacheInventory:()=>call<CacheCategory[]>('cache_inventory'),
  clearCache:(category:string)=>call<void>('clear_cache_category',{category}),
  openExternal:(target:string)=>call<void>('open_external',{target}),
  openDataFolder:()=>call<void>('open_data_folder'),
};
export const errorMessage=(error:unknown)=>error instanceof Error?error.message:String(error);
