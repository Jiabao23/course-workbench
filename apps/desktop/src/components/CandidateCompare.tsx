import { useEffect, useMemo, useState } from 'react';
import type { RecheckCandidate } from '../types';
import { timeLabel } from '../utils';
import { candidateSelection, candidateTextDiff } from './readerQuality';

type Props={onListen?:()=>void;onLoop?:()=>void;looping?:boolean;canPlay?:boolean;candidates:RecheckCandidate[];candidate:RecheckCandidate;locked:boolean;canAdopt:boolean;onCandidate:(candidate:RecheckCandidate)=>void;onAdopt:(id:string)=>void;onAdoptSelected:(ids:string[])=>void;onDiscard:(id:string)=>void};
export default function CandidateCompare({onListen,onLoop,looping,canPlay,candidates,candidate,locked,canAdopt,onCandidate,onAdopt,onAdoptSelected,onDiscard}:Props) {
  const [selected,setSelected]=useState(new Set<string>());
  useEffect(()=>{setSelected(current=>new Set([...current].filter(id=>candidates.some(c=>c.id===id))));},[candidates]);
  const diff=useMemo(()=>candidateTextDiff(candidate.originalText,candidate.segments.map(s=>s.text).join('\n')),[candidate]);
  const selection=useMemo(()=>candidateSelection(candidates,selected),[candidates,selected]);
  const options=candidate.recognitionOptions;
  const engineLabel=[options?.engine,options?.compute_type].filter(value=>typeof value==='string'&&value).join(' · ');
  const metadata=[['beam_size','搜索宽度'],['decoding_policy','识别策略'],['engine_version','引擎版本'],['model_sha256','模型校验值']].filter(([key])=>options?.[key]!=null);
  return <section className="candidate-compare" aria-label="二次识别候选对照">
    <h3>原文与候选对照</h3>
    {candidates.length>1&&<>
      <label className="field">已保存的候选<select aria-label="选择二次识别候选" value={candidate.id} disabled={locked} onChange={e=>{const item=candidates.find(c=>c.id===e.target.value);if(item)onCandidate(item);}}>{candidates.map(c=><option key={c.id} value={c.id}>{timeLabel(c.startMs)}–{timeLabel(c.endMs)} · {new Date(c.createdAt).toLocaleString('zh-CN')}</option>)}</select></label>
      <fieldset className="candidate-selection" disabled={locked||!canAdopt}><legend>选择要一起采用的候选</legend>{candidates.map(c=><label key={c.id}><input type="checkbox" checked={selected.has(c.id)} onChange={()=>setSelected(current=>{const next=new Set(current);if(next.has(c.id))next.delete(c.id);else next.add(c.id);return next;})}/>{timeLabel(c.startMs)}–{timeLabel(c.endMs)}{c.id===candidate.id?' · 正在对照':''}</label>)}</fieldset>
      {selection.error&&<p className="error-text" role="alert">{selection.error}</p>}
      <button disabled={locked||!canAdopt||!selection.ids.length||!!selection.error} onClick={()=>onAdoptSelected(selection.ids)}>采纳所选候选（{selection.ids.length}）</button>
      <p className="hint">所选候选合并为一个新版本。未选候选仍属于旧版本，请在采用前完成对照和选择。</p>
    </>}
    <p className="hint">{candidate.model} · {candidate.device} · {timeLabel(candidate.startMs)}–{timeLabel(candidate.endMs)}</p>
    {onListen&&<div className="actions"><button disabled={locked||!canPlay} onClick={onListen}>回听当前候选区间</button><button disabled={locked||!canPlay} aria-pressed={!!looping} onClick={onLoop}>{looping?'停止候选循环':'循环回听当前候选'}</button></div>}
    {engineLabel&&<p className="hint">{engineLabel}</p>}
    {options&&<details className="candidate-metadata"><summary>识别参数与模型记录</summary>{typeof options.condition_on_previous_text==='boolean'&&<p>{options.condition_on_previous_text?'识别时沿用前文':'独立区间识别，不沿用前文'}</p>}<dl>{metadata.map(([key,label])=><div key={key}><dt>{label}</dt><dd>{String(options[key])}</dd></div>)}</dl></details>}
    <p className="hint">删除线为原文中不同的内容，下划线为候选新增内容。{diff.coarse?'差异较长，按整段标出。':''}</p>
    <h4>原文</h4><p className="candidate-text">{diff.original.length?diff.original.map((part,index)=>part.changed?<del key={index}>{part.text}</del>:<span key={index}>{part.text}</span>):'（此区间无文字）'}</p>
    <h4>候选文字</h4><p className="candidate-text">{diff.candidate.length?diff.candidate.map((part,index)=>part.changed?<ins key={index}>{part.text}</ins>:<span key={index}>{part.text}</span>):'（未识别到文字）'}</p>
    <div className="actions"><button disabled={locked||!canAdopt} onClick={()=>onAdopt(candidate.id)}>采用并生成新版本</button><button disabled={locked} onClick={()=>onDiscard(candidate.id)}>放弃候选</button></div>
  </section>;
}
