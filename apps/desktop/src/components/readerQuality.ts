import type { IntegrityIssue, IntegrityReport, Segment } from '../types';
export function contextInterval(startMs:number,endMs:number,durationMs:number) {
  if(!Number.isFinite(startMs)||!Number.isFinite(endMs)||startMs<0||endMs<startMs)return null;
  const start=Math.max(0,startMs-3000),end=durationMs>0?Math.min(durationMs,endMs+3000):endMs+3000;
  return end>start&&end-start<=120000?{startMs:start,endMs:end}:null;
}
type ReviewableIssue={id:string;severity:string;resolution:{status:string}|null};
export function isPendingIssue(issue:ReviewableIssue){return issue.severity==='warning'&&(!issue.resolution||issue.resolution.status==='pending');}
export function nextPendingId(issues:ReviewableIssue[],currentId:string) {
  const index=issues.findIndex(i=>i.id===currentId);
  for(let step=1;step<=issues.length;step++) {
    const issue=issues[(index+step)%issues.length];
    if(isPendingIssue(issue))return issue.id;
  }
  return null;
}
export function qualitySummary(report:{pendingCount:number;issues?:IntegrityIssue[];diagnosticsAvailable:boolean;review?:unknown;audioCheck:{status:string}}) {
  const counts=report.issues?qualityCounts(report.issues):{doubtGroups:report.pendingCount,evidencePending:0};
  return `文字已生成 · ${counts.doubtGroups?`${counts.doubtGroups} 处待核对`:'暂无待核对疑点'}${counts.evidencePending?` · ${counts.evidencePending} 项检查依据待补全`:''} · ${report.audioCheck.status==='available'?'已检查语音区间':'仅时间轴检查'}${report.diagnosticsAvailable?'':' · 识别诊断缺失'}`;
}

export interface ReviewDraft {text:string;fingerprint:string;issue:IntegrityIssue}
export function bindReviewDraft(current:ReviewDraft|null,issue:IntegrityIssue,fingerprint:string,text:string):ReviewDraft|null {
  if(!text)return null;
  return current?{...current,text}:{text,fingerprint,issue:{...issue}};
}
export function reviewDraftMatches(draft:ReviewDraft|null,report:Pick<IntegrityReport,'fingerprint'|'issues'>|null,issueId:string) {
  return !!draft&&!!report&&draft.fingerprint===report.fingerprint&&draft.issue.id===issueId&&report.issues.some(i=>i.id===draft.issue.id);
}
export function readerVersionAfterRefresh(viewId:string,activeId:string|null){return viewId||activeId||'';}

export interface TimeRange {startMs:number;endMs:number}
export interface IntegrityGroup extends TimeRange {id:string;kind:'temporal'|'evidence'|'other';issues:IntegrityIssue[]}
const evidenceCodes=new Set(['unknownChunks','incompleteChunks','unknownDuration','noDuration']);
export function isEvidenceIssue(issue:Pick<IntegrityIssue,'code'>){return evidenceCodes.has(issue.code);}
function validRange(range:TimeRange){return Number.isFinite(range.startMs)&&Number.isFinite(range.endMs)&&range.startMs>=0&&range.endMs>range.startMs;}

/** Presentation only: original evidence records and their resolutions remain independent. */
export function groupIntegrityIssues(issues:IntegrityIssue[]):IntegrityGroup[] {
  const temporal:IntegrityIssue[]=[],separate:IntegrityGroup[]=[];
  for(const issue of issues){
    if(!isEvidenceIssue(issue)&&validRange(issue))temporal.push(issue);
    else separate.push({id:issue.id,kind:isEvidenceIssue(issue)?'evidence':'other',startMs:issue.startMs,endMs:issue.endMs,issues:[issue]});
  }
  temporal.sort((a,b)=>a.startMs-b.startMs||a.endMs-b.endMs||a.id.localeCompare(b.id));
  const groups:IntegrityGroup[]=[];
  for(const issue of temporal){
    const previous=groups.at(-1);
    if(previous&&issue.startMs<previous.endMs){previous.endMs=Math.max(previous.endMs,issue.endMs);previous.issues.push(issue);}
    else groups.push({id:issue.id,kind:'temporal',startMs:issue.startMs,endMs:issue.endMs,issues:[issue]});
  }
  return [...groups,...separate];
}
export function qualityCounts(issues:IntegrityIssue[]) {
  const pending=issues.filter(isPendingIssue),doubts=pending.filter(i=>!isEvidenceIssue(i));
  return {doubtGroups:groupIntegrityIssues(issues).filter(g=>g.kind!=='evidence'&&g.issues.some(isPendingIssue)).length,pendingDoubts:doubts.length,evidencePending:pending.length-doubts.length};
}

function mergeRanges(ranges:TimeRange[],touching=false):TimeRange[] {
  const merged:TimeRange[]=[];
  for(const range of [...ranges].sort((a,b)=>a.startMs-b.startMs||a.endMs-b.endMs)){
    const previous=merged.at(-1);
    if(previous&&(range.startMs<previous.endMs||(touching&&range.startMs===previous.endMs)))previous.endMs=Math.max(previous.endMs,range.endMs);
    else merged.push({...range});
  }
  return merged;
}
export interface BatchPreview {ranges:TimeRange[];totalMs:number;skipped:Array<TimeRange&{reason:string}>}
export function batchRecheckPreview(issues:IntegrityIssue[],durationMs:number,segments:Segment[]):BatchPreview {
  const preview:BatchPreview={ranges:[],totalMs:0,skipped:[]};
  const groups=groupIntegrityIssues(issues.filter(isPendingIssue)).filter(g=>g.kind==='temporal');
  if(!Number.isFinite(durationMs)||durationMs<=0){
    preview.skipped=groups.map(g=>({startMs:g.startMs,endMs:g.endMs,reason:'缺少本地音频时长'}));return preview;
  }
  // Segment components are prepared once. A binary search finds each boundary;
  // this avoids repeated whole-transcript scans for chained overlapping segments.
  const components=mergeRanges(segments.filter(validRange));
  const contexts:TimeRange[]=[];
  for(const group of groups){
    const range={startMs:Math.max(0,group.startMs-3000),endMs:Math.min(durationMs,group.endMs+3000)};
    if(!validRange(range))preview.skipped.push({startMs:group.startMs,endMs:group.endMs,reason:'区间超出本地音频时长'});
    else contexts.push(range);
  }
  const expanded:TimeRange[]=[];
  for(const context of mergeRanges(contexts,true)){
    let {startMs,endMs}=context;
    let low=0,high=components.length;
    while(low<high){const mid=(low+high)>>>1;if(components[mid].endMs<=startMs)low=mid+1;else high=mid;}
    for(let index=low;index<components.length&&components[index].startMs<endMs;index++){
      startMs=Math.min(startMs,components[index].startMs);endMs=Math.max(endMs,components[index].endMs);
    }
    if(endMs<=startMs||endMs>durationMs){preview.skipped.push({...context,reason:'区间超出本地音频时长'});continue;}
    expanded.push({startMs,endMs});
  }
  for(const range of mergeRanges(expanded,true)){
    const length=range.endMs-range.startMs;
    const reason=length>120000?'含完整片段后超过 120 秒':preview.ranges.length>=5?'本次最多 5 个区间':preview.totalMs+length>300000?'本次总时长最多 300 秒':'';
    if(reason)preview.skipped.push({...range,reason});
    else{preview.ranges.push(range);preview.totalMs+=length;}
  }
  return preview;
}

export interface DiffPart {text:string;changed:boolean}
export function candidateTextDiff(original:string,candidate:string):{original:DiffPart[];candidate:DiffPart[];coarse:boolean} {
  if(original===candidate)return {original:original?[{text:original,changed:false}]:[],candidate:candidate?[{text:candidate,changed:false}]:[],coarse:false};
  const left=Array.from(original),right=Array.from(candidate);let prefix=0,suffix=0;
  while(prefix<left.length&&prefix<right.length&&left[prefix]===right[prefix])prefix++;
  while(suffix<left.length-prefix&&suffix<right.length-prefix&&left[left.length-1-suffix]===right[right.length-1-suffix])suffix++;
  const a=left.slice(prefix,left.length-suffix),b=right.slice(prefix,right.length-suffix);
  const before:DiffPart[]=[],after:DiffPart[]=[];
  const append=(parts:DiffPart[],text:string,changed:boolean)=>{if(!text)return;const previous=parts.at(-1);if(previous&&previous.changed===changed)previous.text+=text;else parts.push({text,changed});};
  append(before,left.slice(0,prefix).join(''),false);append(after,right.slice(0,prefix).join(''),false);
  const coarse=(a.length+1)*(b.length+1)>160000||a.length+b.length>4000;
  if(coarse){append(before,a.join(''),true);append(after,b.join(''),true);}
  else{
    const width=b.length+1,table=new Uint16Array((a.length+1)*width);
    for(let i=a.length-1;i>=0;i--)for(let j=b.length-1;j>=0;j--)table[i*width+j]=a[i]===b[j]?table[(i+1)*width+j+1]+1:Math.max(table[(i+1)*width+j],table[i*width+j+1]);
    let i=0,j=0;
    while(i<a.length||j<b.length){
      if(i<a.length&&j<b.length&&a[i]===b[j]){append(before,a[i++],false);append(after,b[j++],false);}
      else if(i<a.length&&(j===b.length||table[(i+1)*width+j]>=table[i*width+j+1]))append(before,a[i++],true);
      else append(after,b[j++],true);
    }
  }
  append(before,suffix?left.slice(-suffix).join(''):'',false);append(after,suffix?right.slice(-suffix).join(''):'',false);
  return {original:before,candidate:after,coarse};
}

type LoopAudio=Pick<HTMLAudioElement,'currentTime'|'play'|'addEventListener'|'removeEventListener'>;
export function createAudioLoop(audio:LoopAudio,onChange:(range:TimeRange|null)=>void,onError:(message:string)=>void=()=>{}) {
  let range:TimeRange|null=null,disposed=false,generation=0;
  const clear=()=>{generation++;if(range){range=null;onChange(null);}};
  const fail=(error:unknown,epoch:number)=>{if(!disposed&&generation===epoch){clear();onError(error instanceof Error?error.message:String(error));}};
  const update=()=>{if(range&&audio.currentTime>=range.endMs/1000)audio.currentTime=range.startMs/1000;};
  const ended=()=>{if(range){audio.currentTime=range.startMs/1000;const epoch=generation;void audio.play().catch(error=>fail(error,epoch));}};
  const seeking=()=>{if(range&&(audio.currentTime<range.startMs/1000||audio.currentTime>=range.endMs/1000))clear();};
  audio.addEventListener('timeupdate',update);audio.addEventListener('ended',ended);audio.addEventListener('seeking',seeking);
  return {
    async start(next:TimeRange){
      clear();if(disposed||!validRange(next)||next.endMs-next.startMs>120000)return false;
      range={...next};onChange(range);audio.currentTime=range.startMs/1000;const epoch=generation;
      try{await audio.play();return !disposed&&generation===epoch;}catch(error){fail(error,epoch);return false;}
    },
    clear,
    dispose(){clear();disposed=true;audio.removeEventListener('timeupdate',update);audio.removeEventListener('ended',ended);audio.removeEventListener('seeking',seeking);},
  };
}

export async function recheckWithRecovery<T>(action:()=>Promise<T[]>,reload:()=>Promise<T[]>,onRecovered:(items:T[])=>void):Promise<T[]> {
  try{return await action();}
  catch(error){
    try{onRecovered(await reload());}
    catch(refreshError){throw new Error(`${error instanceof Error?error.message:String(error)}；读取已保存候选失败：${refreshError instanceof Error?refreshError.message:String(refreshError)}`);}
    throw error;
  }
}

export function candidateSelection(candidates:Array<TimeRange&{id:string}>,selected:ReadonlySet<string>) {
  const items=candidates.filter(c=>selected.has(c.id)).sort((a,b)=>a.startMs-b.startMs||a.endMs-b.endMs);
  const ids=items.map(c=>c.id);
  if(ids.length!==selected.size)return {ids,error:'候选已变化，请重新选择。'};
  if(items.some((item,index)=>!validRange(item)||(index>0&&item.startMs<items[index-1].endMs)))return {ids,error:'所选候选区间重叠，请为同一时间段保留一个候选。'};
  return {ids,error:''};
}
