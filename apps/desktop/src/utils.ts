import type { Asset, Citation, Segment, SourcePart, Transcript } from './types';
export function timeLabel(ms:number):string {
  const seconds=Math.max(0,Math.floor(ms/1000)),h=Math.floor(seconds/3600),m=Math.floor(seconds/60)%60,s=seconds%60;
  return h?`${h}:${String(m).padStart(2,'0')}:${String(s).padStart(2,'0')}`:`${String(m).padStart(2,'0')}:${String(s).padStart(2,'0')}`;
}
export function durationLabel(ms:number):string { if(ms<60000)return `${Math.max(0,Math.round(ms/1000))} 秒`;const min=Math.round(ms/60000);return min>=60?`${Math.floor(min/60)} 小时 ${min%60} 分`:`${min} 分钟`; }
export function bytesLabel(bytes:number):string {return bytes>=1024**3?`${(bytes/1024**3).toFixed(1)} GB`:`${(bytes/1024**2).toFixed(1)} MB`;}
export function selectedScope(parts:SourcePart[],ids:Set<number>) { const selected=parts.filter(part=>ids.has(part.page));return {pages:selected.map(p=>p.page),durationMs:selected.reduce((sum,p)=>sum+p.durationMs,0)}; }
export function selectedText(segments:Segment[],ids:Set<string>) {const selected=segments.filter(s=>ids.has(s.id));return {segments:selected,chars:selected.reduce((sum,s)=>sum+Array.from(s.text).length,0)};}
export function sourceAt(asset:Asset,ms:number):string|null {
  if(!asset.bvid)return null;
  const url=new URL(`https://www.bilibili.com/video/${encodeURIComponent(asset.bvid)}`);
  url.searchParams.set('p',String(asset.page??1));url.searchParams.set('t',String(Math.floor(Math.max(0,ms)/1000)));return url.toString();
}
export function citationTarget(versions:Transcript[],transcriptId:string,citation:Citation) {
  const version=versions.find(v=>v.id===transcriptId);const segment=version?.segments.find(s=>s.id===citation.segmentId);
  return version&&segment?{version,segment}:null;
}
export function safeFilename(title:string) {return title.replace(/[<>:"/\\|?*\x00-\x1f]/g,'_').slice(0,100).replace(/[. ]+$/,'')||'课程文字稿';}
