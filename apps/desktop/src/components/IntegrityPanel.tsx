import { useEffect, useState } from 'react';
import { api, errorMessage } from '../api';
import type { IntegrityReport } from '../types';
import { timeLabel } from '../utils';
import './IntegrityPanel.css';

type Props = { assetId:string; transcriptId:string; durationMs:number; audioPath:string|null; disabled:boolean; canRetranscribe:boolean; onSeek:(ms:number)=>void; onRetranscribe:()=>void; onDirty:(value:boolean)=>void };
export default function IntegrityPanel({assetId,transcriptId,durationMs,audioPath,disabled,canRetranscribe,onSeek,onRetranscribe,onDirty}:Props) {
  const [report,setReport]=useState<IntegrityReport|null>(null),[error,setError]=useState(''),[draft,setDraft]=useState(''),[busy,setBusy]=useState(false),[refresh,setRefresh]=useState(0),[limit,setLimit]=useState(30);
  useEffect(()=>{let current=true;setReport(null);setError('');
    void api.checkIntegrity(assetId,transcriptId).then(r=>{if(current)setReport(r);}).catch(e=>{if(current)setError(errorMessage(e));});
    return()=>{current=false;};
  },[assetId,transcriptId,durationMs,audioPath,refresh]);
  useEffect(()=>{onDirty(!!draft.trim());return()=>onDirty(false);},[draft,onDirty]);
  async function review(){if(!report)return;setBusy(true);setError('');try {setReport(await api.reviewIntegrity(assetId,transcriptId,report.fingerprint,draft));setDraft('');}catch(e){setError(errorMessage(e));}finally{setBusy(false);}}
  const locked=disabled||busy;
  return <details className="integrity-panel">
    <summary>转录完整性核对 · {report?(report.review?'已记录人工结论':report.issues.length?`${report.issues.length} 项待核对`:'未发现明显异常'):error?'检查失败':'正在检查'}</summary>
    {error&&<p className="error-text" role="alert">{error}</p>}
    {report&&<>
      <p className="hint">{report.limitations}</p>
      <p>{report.segmentCount} 段 · {report.durationMs?`时间轴覆盖 ${(report.coveredMs/report.durationMs*100).toFixed(1)}%`:'来源时长未知'}{report.chunkTotal?` · 对应任务 ${report.chunkDone}/${report.chunkTotal} 块`:''}</p>
      <div className="integrity-issues">{report.issues.slice(0,limit).map((issue,index)=><div key={`${issue.code}-${index}`}><button className="timestamp" disabled={locked} onClick={()=>onSeek(issue.startMs)} aria-label={`核对 ${timeLabel(issue.startMs)} ${issue.code}`}>{timeLabel(issue.startMs)}{issue.endMs>issue.startMs?`–${timeLabel(issue.endMs)}`:''}</button><span>{issue.message}</span></div>)}</div>
      {report.issues.length>limit&&<button onClick={()=>setLimit(n=>n+30)}>再显示 30 项</button>}
      {report.review&&<div className="integrity-review"><strong>人工核对记录 · {new Date(report.review.reviewedAt).toLocaleString('zh-CN')}</strong><p>{report.review.note}</p><small>原有疑点保留；人工记录不改变规则检测依据。</small></div>}
      <label className="field">核对结论<textarea aria-label="完整性核对结论" disabled={locked} maxLength={4000} rows={2} value={draft} onChange={e=>setDraft(e.target.value)} placeholder="回听后记录：哪些是静音、哪些需要补转，或已对照原课确认的范围。"/></label>
      <div className="actions"><button disabled={locked||!draft.trim()} onClick={()=>void review()}>保存核对记录</button><button disabled={locked||!!draft.trim()} onClick={()=>setRefresh(n=>n+1)}>重新检查</button>{canRetranscribe&&<button disabled={locked||!!draft.trim()} onClick={onRetranscribe}>重新转写为新版本</button>}</div>
    </>}
    {!report&&error&&<button onClick={()=>setRefresh(n=>n+1)}>重试检查</button>}
  </details>;
}
