import { useEffect, useMemo, useRef, useState } from 'react';
import { api, errorMessage } from '../api';
import type { IntegrityIssue, IntegrityReport, RecheckCandidate, Segment, Transcript } from '../types';
import { timeLabel } from '../utils';
import { batchRecheckPreview, bindReviewDraft, contextInterval, groupIntegrityIssues, isEvidenceIssue, isPendingIssue, nextPendingId, qualityCounts, qualitySummary, recheckWithRecovery, reviewDraftMatches, type BatchPreview, type ReviewDraft, type TimeRange } from './readerQuality';
import CandidateCompare from './CandidateCompare';
import './IntegrityPanel.css';

type Props = {
  assetId:string;transcriptId:string;durationMs:number;segments:Segment[];audioPath:string|null;
  disabled:boolean;canRetranscribe:boolean;canAdopt:boolean;looping:boolean;
  onSeek:(ms:number)=>void;onLoop:(range:TimeRange)=>void;onClearLoop:()=>void;
  onRetranscribe:()=>void;onDirty:(value:boolean)=>void;onBusy:(value:boolean)=>void;
  onSummary:(value:string)=>void;onAdopted:(version:Transcript)=>Promise<void>;onGetAudio?:()=>void;
};
const orderedIssues=(issues:IntegrityIssue[])=>groupIntegrityIssues(issues).flatMap(group=>group.issues);
const issueStatus=(issue:IntegrityIssue)=>issue.resolution?.status==='revised'?'已修订（请核对新版本）':issue.resolution?.status==='confirmed'?'已确认正常':issue.severity==='info'?'信息提示':isEvidenceIssue(issue)?'依据待补全':'待核对';

export default function IntegrityPanel({assetId,transcriptId,durationMs,segments,audioPath,disabled,canRetranscribe,canAdopt,looping,onSeek,onLoop,onClearLoop,onRetranscribe,onDirty,onBusy,onSummary,onAdopted,onGetAudio}:Props) {
  const [report,setReport]=useState<IntegrityReport|null>(null),[error,setError]=useState(''),[reviewDraft,setReviewDraft]=useState<ReviewDraft|null>(null),[busy,setBusy]=useState(''),[refresh,setRefresh]=useState(0);
  const [issueId,setIssueId]=useState(''),[pendingOnly,setPendingOnly]=useState(true),[candidate,setCandidate]=useState<RecheckCandidate|null>(null),[candidates,setCandidates]=useState<RecheckCandidate[]>([]),[candidatesLoading,setCandidatesLoading]=useState(true);
  const [preview,setPreview]=useState<BatchPreview|null>(null);
  const running=useRef(false),draftRef=useRef<ReviewDraft|null>(null);
  const [checking,setChecking]=useState(true),[reportFresh,setReportFresh]=useState(false);
  const draft=reviewDraft?.text??'';

  useEffect(()=>{
    let current=true;setChecking(true);setReportFresh(false);setError('');onSummary('正在检查文字时间轴');
    void api.checkIntegrity(assetId,transcriptId).then(r=>{if(current){setReport(r);setReportFresh(true);setIssueId(draftRef.current?.issue.id??nextPendingId(orderedIssues(r.issues),'')??'');}}).catch(e=>{if(current){setError(errorMessage(e));onSummary('检查失败 · 打开核对重试');}}).finally(()=>{if(current)setChecking(false);});
    return()=>{current=false;};
  },[assetId,transcriptId,durationMs,audioPath,refresh,onSummary]);
  useEffect(()=>{
    let current=true;setCandidatesLoading(true);
    void api.listRecheckCandidates(assetId,transcriptId).then(items=>{if(current){setCandidates(items);setCandidate(items[0]??null);}}).catch(e=>{if(current)setError(errorMessage(e));}).finally(()=>{if(current)setCandidatesLoading(false);});
    return()=>{current=false;};
  },[assetId,transcriptId]);
  useEffect(()=>{if(report)onSummary(qualitySummary(report));},[report,onSummary]);
  useEffect(()=>{onDirty(!!draft.trim()||!!busy);return()=>onDirty(false);},[draft,busy,onDirty]);
  useEffect(()=>{onBusy(!!busy);return()=>onBusy(false);},[busy,onBusy]);
  useEffect(()=>()=>{if(running.current)void api.cancelQuality().catch(()=>{});},[]);
  useEffect(()=>{onClearLoop();return onClearLoop;},[issueId,report?.fingerprint,onClearLoop]);
  useEffect(()=>{setPreview(null);},[report,segments,durationMs]);

  const groups=useMemo(()=>groupIntegrityIssues(report?.issues??[]),[report]);
  const counts=useMemo(()=>qualityCounts(report?.issues??[]),[report]);
  const visibleGroups=groups.filter(group=>!pendingOnly||group.issues.some(isPendingIssue));
  const group=visibleGroups.find(item=>item.issues.some(i=>i.id===issueId));
  const issue=reviewDraft?.issue??group?.issues.find(i=>i.id===issueId);
  const interval=issue&&!isEvidenceIssue(issue)&&issue.endMs>issue.startMs?contextInterval(issue.startMs,issue.endMs,durationMs):null;
  const locked=disabled||!!busy||checking;
  const staleDraft=!!reviewDraft&&(!reportFresh||!reviewDraftMatches(reviewDraft,report,issueId));
  const selectionLocked=locked||!!draft.trim()||!!candidate;
  const recheckLocked=selectionLocked||candidatesLoading||!audioPath||!canAdopt;

  function setDraft(text:string){
    const next=text&&issue&&report?bindReviewDraft(draftRef.current,issue,report.fingerprint,text):null;
    draftRef.current=next;setReviewDraft(next);
  }
  function discardDraft(){setDraft('');if(report)setIssueId(report.issues.some(i=>i.id===issueId&&(!pendingOnly||isPendingIssue(i)))?issueId:nextPendingId(orderedIssues(report.issues),'')??'');}
  function applyCandidates(items:RecheckCandidate[]){setCandidates(items);setCandidate(current=>items.find(item=>item.id===current?.id)??items[0]??null);}
  async function run(label:string,action:()=>Promise<void>){
    if(running.current)return;
    running.current=true;setBusy(label);setError('');onClearLoop();
    try{await action();}
    catch(e){const message=errorMessage(e);setError(message.includes('任务已取消')?`${message}。原文未修改；已完成的候选可继续对照。`:message);}
    finally{running.current=false;setBusy('');}
  }
  async function recheck(action:()=>Promise<RecheckCandidate[]>){
    const items=await recheckWithRecovery(action,()=>api.listRecheckCandidates(assetId,transcriptId),applyCandidates);
    applyCandidates([...candidates,...items.filter(item=>!candidates.some(existing=>existing.id===item.id))]);
  }
  async function review(status:'confirmed'|'pending'){
    if(!report||!issue||!reviewDraft||!draft.trim()||staleDraft||locked)return;
    const bound=reviewDraft;
    await run('保存核对',async()=>{
      const updated=await api.reviewIntegrityIssue(assetId,transcriptId,bound.fingerprint,bound.issue.id,status,bound.text);
      setReport(updated);setDraft('');
      if(status==='confirmed')setIssueId(nextPendingId(orderedIssues(updated.issues),issue.id)??issue.id);
    });
  }
  const overallReview=report?.review??report?.historicalReview;

  return <section className="integrity-panel" aria-label="转录质量核对">
    {error&&<p className="error-text" role="alert">{error}</p>}
    {report&&<>
      <p className="quality-count">{counts.doubtGroups} 处待核对疑点 · {counts.pendingDoubts} 条疑点证据</p>
      {!!counts.evidencePending&&<p className="quality-evidence-count">{counts.evidencePending} 项检查依据待补全，单独列示，不计入疑点数量。</p>}
      <p className="hint">{report.limitations}</p>
      {!report.diagnosticsAvailable&&<p className="hint">识别诊断缺失：当前版本没有可用的识别诊断分数，仍需结合原音核对。</p>}
      <p className="hint">{report.audioCheck.message}</p>
      {report.audioCheck.status==='unavailable'&&<p className="hint">如提示缺少语音检测依赖，请在设置 → 资料与工具中配置「语音检测扩展目录」。</p>}
      <div className="actions"><button disabled={selectionLocked||!audioPath} onClick={()=>void run('正在本机检测语音区间',async()=>{const updated=await api.detectSpeech(assetId,transcriptId);setReport(updated);setIssueId(nextPendingId(orderedIssues(updated.issues),'')??'');})}>检测语音区间（CPU）</button>{!audioPath&&onGetAudio&&<button disabled={locked} onClick={onGetAudio}>获取回听音频</button>}</div>
      {!audioPath&&<p className="hint">没有本地音频，仅检查文字时间轴。</p>}
      <label className="quality-filter"><input type="checkbox" checked={pendingOnly} disabled={selectionLocked} onChange={e=>{setPendingOnly(e.target.checked);setIssueId(e.target.checked?nextPendingId(orderedIssues(report.issues),'')??'':groups[0]?.issues[0]?.id??'');}}/>只看待处理（含检查依据提示）</label>
      {pendingOnly&&!visibleGroups.length&&<p className="hint">暂无待处理记录。取消勾选可查看已处理和信息提示记录。</p>}
      {!!visibleGroups.length&&<label className="field">核对区间与检查依据<select aria-label="选择核对区间" disabled={selectionLocked} value={group?.id??''} onChange={e=>{const target=visibleGroups.find(g=>g.id===e.target.value);if(target)setIssueId((target.issues.find(isPendingIssue)??target.issues[0]).id);}}>
        <option value="" disabled>选择核对区间</option>
        {(['temporal','other','evidence'] as const).map(kind=>visibleGroups.some(g=>g.kind===kind)&&<optgroup key={kind} label={kind==='evidence'?'检查依据待补全':kind==='temporal'?'时间区间':'其他文字与时间轴记录'}>{visibleGroups.filter(g=>g.kind===kind).map(g=><option key={g.id} value={g.id}>{kind==='temporal'?`${timeLabel(g.startMs)}–${timeLabel(g.endMs)} · ${g.issues.length} 条证据`:g.issues[0].message}</option>)}</optgroup>)}
      </select></label>}
      {group&&group.issues.length>1&&<><label className="field">本组证据<select aria-label="选择本组证据" disabled={selectionLocked} value={issueId} onChange={e=>setIssueId(e.target.value)}>{group.issues.map(i=><option key={i.id} value={i.id}>{issueStatus(i)} · {i.message}</option>)}</select></label><p className="hint">相交区间合并展示。结论仅保存到当前证据，同组其他证据仍需单独核对。</p></>}
      {issue&&<article className="quality-issue">
        <p><strong>{issueStatus(issue)}</strong>{issue.endMs>issue.startMs&&<> · {timeLabel(issue.startMs)}–{timeLabel(issue.endMs)}</>}</p><p>{issue.message}</p>
        {issue.severity==='info'&&<p className="hint">未检测到讲话，但可回听复核；不计入待核对数量。</p>}
        {isEvidenceIssue(issue)&&<p className="hint">这是检查依据提示，不能据此认定文字转写有误。</p>}
        <div className="actions">
          {!isEvidenceIssue(issue)&&issue.endMs>issue.startMs&&<button disabled={locked||!!candidate} onClick={()=>onSeek(Math.max(0,issue.startMs-3000))}>回听前后文</button>}
          {interval&&<button disabled={locked||!audioPath||!!candidate} aria-pressed={looping} onClick={()=>looping?onClearLoop():onLoop(interval)}>{looping?'停止区间循环':'循环回听此区间'}</button>}
          <button disabled={selectionLocked||!report.pendingCount} onClick={()=>setIssueId(nextPendingId(orderedIssues(report.issues),issue.id)??issue.id)}>下一项待处理</button>
        </div>
        {issue.resolution&&<p className="hint">{issue.resolution.note} · {new Date(issue.resolution.reviewedAt).toLocaleString('zh-CN')}</p>}
        {staleDraft&&<p className="error-text" role="alert">此理由的检查依据已变化或尚未重新核实，不能提交。已保留原疑点与草稿；请复制需要的内容后放弃理由，再按新依据核对。</p>}
        <label className="field">本项核对理由<textarea aria-label="本项核对理由" readOnly={staleDraft} disabled={locked||!!candidate} maxLength={4000} rows={3} value={draft} onChange={e=>setDraft(e.target.value)} placeholder="回听后说明正常停顿，或记录仍需修改的内容。"/></label>
        <div className="actions"><button disabled={locked||staleDraft||!draft.trim()||!!candidate} onClick={()=>void review('confirmed')}>确认正常</button><button disabled={locked||staleDraft||!draft.trim()||!!candidate} onClick={()=>void review('pending')}>保存为待核对</button>{!!draft&&<button disabled={locked} onClick={discardDraft}>放弃理由</button>}</div>
        {!isEvidenceIssue(issue)&&<><button className="quality-recheck" disabled={recheckLocked||!interval} onClick={()=>interval&&void run('正在局部二次识别',()=>recheck(async()=>[await api.recheckInterval(assetId,transcriptId,interval.startMs,interval.endMs)]))}>局部二次识别</button>
          <p className="hint">{!canAdopt?'请先保存笔记或校对内容，并切换到当前文字版本。':interval?`含前后各 3 秒上下文：${timeLabel(interval.startMs)}–${timeLabel(interval.endMs)}。识别范围会扩展到完整片段边界，最终不超过 120 秒；采用候选后生成新版本。`:'该疑点无有效时间范围，或含上下文超过 120 秒，请手动校对或重新转写。'}</p></>}
      </article>}
      <div className="quality-batch">
        <button disabled={recheckLocked||!counts.pendingDoubts} aria-expanded={!!preview} onClick={()=>setPreview(batchRecheckPreview(report.issues,durationMs,segments))}>预览批量二次识别</button>
        {preview&&<section aria-label="批量二次识别预览"><h3>本次识别范围</h3><p className="hint">已合并相交区间，包含前后各 3 秒及完整边界片段。最多 5 个区间、每个 120 秒、总计 300 秒。</p>
          {preview.ranges.length?<ol>{preview.ranges.map(range=><li key={`${range.startMs}-${range.endMs}`}>{timeLabel(range.startMs)}–{timeLabel(range.endMs)} · {((range.endMs-range.startMs)/1000).toFixed(1)} 秒</li>)}</ol>:<p className="hint">没有符合本次限制的待核对区间。</p>}
          <p>共 {preview.ranges.length} 个区间 · {(preview.totalMs/1000).toFixed(1)} 秒</p>
          {!!preview.skipped.length&&<details><summary>{preview.skipped.length} 个区间未纳入本次</summary><ul>{preview.skipped.map((item,index)=><li key={index}>{timeLabel(item.startMs)}–{timeLabel(item.endMs)}：{item.reason}</li>)}</ul></details>}
          <p className="hint">点击开始后才会在本机识别。生成候选后逐项对照，再选择是否采用。</p>
          <div className="actions"><button disabled={recheckLocked||!preview.ranges.length} onClick={()=>{const ranges=preview.ranges;setPreview(null);void run('正在批量二次识别',()=>recheck(()=>api.recheckBatch(assetId,transcriptId,ranges)));}}>开始本次识别</button><button disabled={locked} onClick={()=>setPreview(null)}>收起预览</button></div>
        </section>}
      </div>
      {candidate&&<CandidateCompare candidates={candidates} candidate={candidate} locked={locked} canAdopt={canAdopt} onCandidate={item=>{onClearLoop();setCandidate(item);}}
        canPlay={!!audioPath} looping={looping} onListen={()=>onSeek(candidate.startMs)} onLoop={()=>looping?onClearLoop():onLoop({startMs:candidate.startMs,endMs:candidate.endMs})}
        onAdopt={id=>void run('采用为新版本',async()=>{const version=await api.adoptCandidate(id);setCandidate(null);await onAdopted(version);})}
        onAdoptSelected={ids=>void run('合并采用为新版本',async()=>{const version=await api.adoptCandidates(ids);setCandidate(null);await onAdopted(version);})}
        onDiscard={id=>void run('放弃候选',async()=>{await api.discardCandidate(id);applyCandidates(candidates.filter(c=>c.id!==id));})}/>}
      {!report.issues.length&&<p className="hint">未发现明显异常；这不等于逐字内容已核实。</p>}
      {overallReview&&<details className="integrity-review"><summary>历史整体核对记录</summary><p>{overallReview.note}</p><small>整体记录不清除任何待核对疑点。</small></details>}
      <div className="actions"><button disabled={selectionLocked} onClick={()=>setRefresh(n=>n+1)}>重新检查时间轴</button>{canRetranscribe&&<button disabled={selectionLocked} onClick={onRetranscribe}>重新转写为新版本</button>}</div>
      <p className="hint">需要修订时，请校对保存新版本，或采用二次识别候选。旧版疑点及引用仍保留。</p>
    </>}
    {error&&!reportFresh&&<button disabled={locked} onClick={()=>setRefresh(n=>n+1)}>重试检查</button>}
    {busy&&<div className="quality-progress" role="status"><span>{busy}</span>{(busy.includes('检测')||busy.includes('识别'))&&<button onClick={()=>void api.cancelQuality().catch(e=>setError(errorMessage(e)))}>取消任务</button>}</div>}
  </section>;
}
