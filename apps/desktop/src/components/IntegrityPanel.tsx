import { useEffect, useRef, useState } from 'react';
import { api, errorMessage } from '../api';
import type { IntegrityReport, RecheckCandidate, Transcript } from '../types';
import { timeLabel } from '../utils';
import { contextInterval, isPendingIssue, nextPendingId, qualitySummary, bindReviewDraft, reviewDraftMatches, type ReviewDraft } from './readerQuality';
import './IntegrityPanel.css';

type Props = { assetId:string; transcriptId:string; durationMs:number; audioPath:string|null; disabled:boolean; canRetranscribe:boolean; canAdopt:boolean; onSeek:(ms:number)=>void; onRetranscribe:()=>void; onDirty:(value:boolean)=>void; onBusy:(value:boolean)=>void; onSummary:(value:string)=>void; onAdopted:(version:Transcript)=>Promise<void>; onGetAudio?:()=>void };
export default function IntegrityPanel({assetId,transcriptId,durationMs,audioPath,disabled,canRetranscribe,canAdopt,onSeek,onRetranscribe,onDirty,onBusy,onSummary,onAdopted,onGetAudio}:Props) {
  const [report,setReport]=useState<IntegrityReport|null>(null),[error,setError]=useState(''),[reviewDraft,setReviewDraft]=useState<ReviewDraft|null>(null),[busy,setBusy]=useState(''),[refresh,setRefresh]=useState(0);
  const [issueId,setIssueId]=useState(''),[pendingOnly,setPendingOnly]=useState(true),[candidate,setCandidate]=useState<RecheckCandidate|null>(null),[candidates,setCandidates]=useState<RecheckCandidate[]>([]),[candidatesLoading,setCandidatesLoading]=useState(true);
  const running=useRef(false),draftRef=useRef<ReviewDraft|null>(null);
  const [checking,setChecking]=useState(true),[reportFresh,setReportFresh]=useState(false);
  const draft=reviewDraft?.text??'';
  useEffect(()=>{let current=true;setChecking(true);setReportFresh(false);setError('');onSummary('正在检查文字时间轴');
    void api.checkIntegrity(assetId,transcriptId).then(r=>{if(current){setReport(r);setReportFresh(true);setIssueId(draftRef.current?.issue.id??nextPendingId(r.issues,'')??'');}}).catch(e=>{if(current){setError(errorMessage(e));onSummary('检查失败 · 打开核对重试');}}).finally(()=>{if(current)setChecking(false);});
    return()=>{current=false;};
  },[assetId,transcriptId,durationMs,audioPath,refresh,onSummary]);
  useEffect(()=>{let current=true;setCandidatesLoading(true);void api.listRecheckCandidates(assetId,transcriptId).then(items=>{if(current){setCandidates(items);setCandidate(items[0]??null);}}).catch(e=>{if(current)setError(errorMessage(e));}).finally(()=>{if(current)setCandidatesLoading(false);});return()=>{current=false;};},[assetId,transcriptId]);
  useEffect(()=>{if(report)onSummary(qualitySummary(report));},[report,onSummary]);
  useEffect(()=>{onDirty(!!draft.trim()||!!busy);return()=>onDirty(false);},[draft,busy,onDirty]);
  useEffect(()=>{onBusy(!!busy);return()=>onBusy(false);},[busy,onBusy]);
  useEffect(()=>()=>{if(running.current)void api.cancelQuality().catch(()=>{});},[]);
  const issue=reviewDraft?.issue??report?.issues.find(i=>i.id===issueId&&(!pendingOnly||isPendingIssue(i)));
  const interval=issue?contextInterval(issue.startMs,issue.endMs,durationMs):null;
  const locked=disabled||!!busy||checking;
  const staleDraft=!!reviewDraft&&(!reportFresh||!reviewDraftMatches(reviewDraft,report,issueId));
  function setDraft(text:string){
    const next=text&&issue&&report?bindReviewDraft(draftRef.current,issue,report.fingerprint,text):null;
    draftRef.current=next;setReviewDraft(next);
  }
  function discardDraft(){setDraft('');if(report)setIssueId(report.issues.some(i=>i.id===issueId&&(!pendingOnly||isPendingIssue(i)))?issueId:nextPendingId(report.issues,'')??'');}
  async function run(label:string,action:()=>Promise<void>){setBusy(label);setError('');running.current=true;try{await action();}catch(e){const message=errorMessage(e);setError(message.includes('任务已取消')?'任务已取消，原文未修改；可以重新执行。':message);}finally{running.current=false;setBusy('');}}
  async function review(status:'confirmed'|'pending'){if(!report||!issue||!reviewDraft||!draft.trim()||staleDraft||locked)return;const bound=reviewDraft;await run('保存核对',async()=>{const updated=await api.reviewIntegrityIssue(assetId,transcriptId,bound.fingerprint,bound.issue.id,status,bound.text);setReport(updated);setDraft('');if(status==='confirmed')setIssueId(nextPendingId(updated.issues,issue.id)??issue.id);});}
  const overallReview=report?.review??report?.historicalReview;
  const visibleIssues=report?.issues.filter(i=>!pendingOnly||isPendingIssue(i))??[];
  return <section className="integrity-panel" aria-label="转录质量核对">
    {error&&<p className="error-text" role="alert">{error}</p>}
    {report&&<>
      <p className="quality-count">{report.pendingCount} 处待核对 · {report.issues.length} 处检查记录</p>
      <p className="hint">{report.limitations}</p>{!report.diagnosticsAvailable&&<p className="hint">识别诊断缺失：当前版本没有可用的识别诊断分数，仍需结合原音核对。</p>}
      <p className="hint">{report.audioCheck.message}</p>{report.audioCheck.status==='unavailable'&&<p className="hint">如提示缺少语音检测依赖，请在设置 → 资料与工具中配置「语音检测扩展目录」。</p>}
      <div className="actions"><button disabled={locked||!!draft.trim()||!!candidate||!audioPath} onClick={()=>void run('正在本机检测语音区间',async()=>{const updated=await api.detectSpeech(assetId,transcriptId);setReport(updated);setIssueId(nextPendingId(updated.issues,'')??'');})}>检测语音区间（CPU）</button>{!audioPath&&onGetAudio&&<button disabled={locked} onClick={onGetAudio}>获取回听音频</button>}</div>
      {!audioPath&&<p className="hint">没有本地音频，仅检查文字时间轴。</p>}
      <label className="quality-filter"><input type="checkbox" checked={pendingOnly} disabled={locked||!!draft.trim()||!!candidate} onChange={e=>{setPendingOnly(e.target.checked);setIssueId(e.target.checked?nextPendingId(report.issues,'')??'':report.issues[0]?.id??'');}}/>只看待核对（取消勾选查看全部记录）</label>
      {pendingOnly&&!visibleIssues.length&&<p className="hint">暂无待核对疑点。取消勾选可查看已处理和信息提示记录。</p>}{!!visibleIssues.length&&<label className="field">疑点<select aria-label="选择核对疑点" disabled={locked||!!draft.trim()||!!candidate} value={visibleIssues.some(i=>i.id===issueId)?issueId:''} onChange={e=>setIssueId(e.target.value)}><option value="" disabled>选择疑点</option>{visibleIssues.map(i=><option key={i.id} value={i.id}>{timeLabel(i.startMs)} · {i.message}</option>)}</select></label>}
      {issue&&<article className="quality-issue"><p><strong>{issue.resolution?.status==='revised'?'已修订（请核对新版本）':issue.resolution?.status==='confirmed'?'已确认正常':issue.severity==='info'?'信息提示':'待核对'}</strong> · {timeLabel(issue.startMs)}–{timeLabel(issue.endMs)}</p><p>{issue.message}</p>{issue.severity==='info'&&<p className="hint">未检测到讲话，但可回听复核；不计入待核对数量。</p>}
        <div className="actions"><button disabled={locked} onClick={()=>onSeek(Math.max(0,issue.startMs-3000))}>回听前后文</button><button disabled={locked||!!draft.trim()||!!candidate||!report.pendingCount} onClick={()=>setIssueId(nextPendingId(report.issues,issue.id)??issue.id)}>下一处待核对</button></div>
        {issue.resolution&&<p className="hint">{issue.resolution.note} · {new Date(issue.resolution.reviewedAt).toLocaleString('zh-CN')}</p>}
        {staleDraft&&<p className="error-text" role="alert">此理由的检查依据已变化或尚未重新核实，不能提交。已保留原疑点与草稿；请复制需要的内容后放弃理由，再按新依据核对。</p>}
        <label className="field">本项核对理由<textarea aria-label="本项核对理由" readOnly={staleDraft} disabled={locked||!!candidate} maxLength={4000} rows={3} value={draft} onChange={e=>setDraft(e.target.value)} placeholder="回听后说明正常停顿，或记录仍需修改的内容。"/></label>
        <div className="actions"><button disabled={locked||staleDraft||!draft.trim()||!!candidate} onClick={()=>void review('confirmed')}>确认正常</button><button disabled={locked||staleDraft||!draft.trim()||!!candidate} onClick={()=>void review('pending')}>保存为待核对</button>{!!draft&&<button disabled={locked} onClick={discardDraft}>放弃理由</button>}</div>
        <button className="quality-recheck" disabled={locked||candidatesLoading||!audioPath||!interval||!!draft.trim()||!!candidate||!canAdopt} onClick={()=>interval&&void run('正在局部二次识别',async()=>{const item=await api.recheckInterval(assetId,transcriptId,interval.startMs,interval.endMs);setCandidate(item);setCandidates(items=>[...items,item]);})}>局部二次识别</button>
        <p className="hint">{!canAdopt?'请先保存笔记或校对内容，并切换到当前文字版本。':interval?`含前后各 3 秒上下文：${timeLabel(interval.startMs)}–${timeLabel(interval.endMs)}。识别范围会扩展到完整片段边界，最终不超过 120 秒；采用候选后生成新版本。`:'该疑点含上下文超过 120 秒，请手动校对或重新转写。'}</p>
      </article>}
      {candidate&&<section className="candidate-compare" aria-label="二次识别候选对照"><h3>原文与候选对照</h3>{candidates.length>1&&<label className="field">已保存的候选<select value={candidate.id} disabled={locked} onChange={e=>setCandidate(candidates.find(c=>c.id===e.target.value)??null)}>{candidates.map(c=><option key={c.id} value={c.id}>{timeLabel(c.startMs)}–{timeLabel(c.endMs)} · {new Date(c.createdAt).toLocaleString('zh-CN')}</option>)}</select></label>}<p className="hint">{candidate.model} · {candidate.device} · {timeLabel(candidate.startMs)}–{timeLabel(candidate.endMs)}</p><h4>原文</h4><p>{candidate.originalText||'（此区间无文字）'}</p><h4>候选文字</h4><p>{candidate.segments.map(s=>s.text).join('\n')||'（未识别到文字）'}</p><div className="actions"><button disabled={locked||!canAdopt} onClick={()=>void run('采用为新版本',async()=>{const version=await api.adoptCandidate(candidate.id);setCandidate(null);await onAdopted(version);})}>采用并生成新版本</button><button disabled={locked} onClick={()=>void run('放弃候选',async()=>{await api.discardCandidate(candidate.id);const remaining=candidates.filter(c=>c.id!==candidate.id);setCandidates(remaining);setCandidate(remaining[0]??null);})}>放弃候选</button></div></section>}
      {!report.issues.length&&<p className="hint">未发现明显异常；这不等于逐字内容已核实。</p>}
      {overallReview&&<details className="integrity-review"><summary>历史整体核对记录</summary><p>{overallReview.note}</p><small>整体记录不清除任何待核对疑点。</small></details>}
      <div className="actions"><button disabled={locked||!!draft.trim()||!!candidate} onClick={()=>setRefresh(n=>n+1)}>重新检查时间轴</button>{canRetranscribe&&<button disabled={locked||!!draft.trim()||!!candidate} onClick={onRetranscribe}>重新转写为新版本</button>}</div>
      <p className="hint">需要修订时，请校对保存新版本，或采用二次识别候选。旧版疑点及引用仍保留。</p>
    </>}
    {error&&!reportFresh&&<button onClick={()=>setRefresh(n=>n+1)}>重试检查</button>}
    {busy&&<div className="quality-progress" role="status"><span>{busy}</span>{(busy.includes('检测')||busy.includes('识别'))&&<button onClick={()=>void api.cancelQuality().catch(e=>setError(errorMessage(e)))}>取消任务</button>}</div>}
  </section>;
}
