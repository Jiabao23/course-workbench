import type { IntegrityIssue, IntegrityReport } from '../types';
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
export function qualitySummary(report:{pendingCount:number;diagnosticsAvailable:boolean;review?:unknown;audioCheck:{status:string}}) {
  return `文字已生成 · ${report.pendingCount?`${report.pendingCount} 处待核对`:'暂无待核对疑点'} · ${report.audioCheck.status==='available'?'已检查语音区间':'仅时间轴检查'}${report.diagnosticsAvailable?'':' · 识别诊断缺失'}`;
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
