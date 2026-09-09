import { useRef,useState } from 'react';
import { open } from '@tauri-apps/plugin-dialog';
import { FileUp,Link2,ArrowRight,RefreshCw } from 'lucide-react';
import { api,errorMessage } from '../api';
import type { SourcePreview,SubtitleStatus } from '../types';
import { durationLabel,selectedScope,timeLabel } from '../utils';
import { Modal,Spinner } from './Common';
const statusLabel:Record<SubtitleStatus,string>={available:'可提取字幕',absent:'没有可读字幕',loginRequired:'字幕需要登录',failed:'字幕请求失败',unchecked:'处理前检查字幕'};
export default function ImportDialog({onClose,onImported}:{onClose:()=>void;onImported:()=>Promise<void>}) {
  const [source,setSource]=useState(''),[preview,setPreview]=useState<SourcePreview|null>(null),[pages,setPages]=useState(new Set<number>()),[mode,setMode]=useState('auto'),[busy,setBusy]=useState(''),[error,setError]=useState('');
  const requestId=useRef(0);
  const changeSource=(value:string)=>{requestId.current++;setSource(value);setPreview(null);setPages(new Set());setError('');setBusy('');};
  async function analyze(value=source){
    if(!value.trim())return;const id=++requestId.current;setBusy('正在读取课程信息');setError('');
    try{const result=await api.probeSource(value.trim());if(requestId.current!==id)return;setPreview(result);setPages(new Set(result.parts.length?[result.parts[0].page]:[]));}
    catch(e){if(requestId.current===id)setError(errorMessage(e));}finally{if(requestId.current===id)setBusy('');}
  }
  async function chooseFile(){try{const path=await open({multiple:false,filters:[{name:'课程资料',extensions:['mp3','m4a','mp4','wav','flac','mkv','webm','ogg','opus','aac','mov','wma','srt','vtt','json']}]});if(typeof path==='string'){changeSource(path);await analyze(path);}}catch(e){setError(errorMessage(e));}}
  const scope=selectedScope(preview?.parts??[],pages);
  async function submit(){if(!preview||!scope.pages.length)return;setBusy('正在添加任务');setError('');try{await api.createJobs(preview.source,scope.pages,mode);await onImported();onClose();}catch(e){setError(errorMessage(e));}finally{setBusy('');}}
  async function checkPart(page:number){if(!preview)return;setBusy(`正在检查 P${page} 的字幕`);setError('');const id=requestId.current;try{const part=await api.probePart(preview.source,page);if(id===requestId.current)setPreview(current=>current?{...current,parts:current.parts.map(p=>p.page===page?part:p)}:current);}catch(e){setError(errorMessage(e));}finally{if(id===requestId.current)setBusy('');}}
  return <Modal title="导入课程" wide onClose={()=>{if(!busy){requestId.current++;onClose();}}}>
    <div className="modal-content import-content">
      <p className="muted">先读取课程信息，再选择要处理的内容。</p>
      <label className="field">B 站链接 / BV 号 / 本地文件路径<div className="input-action"><Link2 size={18}/><input autoFocus value={source} onChange={e=>changeSource(e.target.value)} placeholder="https://www.bilibili.com/video/BV…" onKeyDown={e=>{if(e.key==='Enter'&&!busy)void analyze();}}/><button className="primary" disabled={!!busy||!source.trim()} onClick={()=>void analyze()}>读取<ArrowRight size={15}/></button></div></label>
      <button className="quiet" disabled={!!busy} onClick={()=>void chooseFile()}><FileUp size={17}/>选择本地音视频或字幕</button>
      {error&&<div className="error-banner" role="alert">{error}</div>}
      {preview&&<section className="import-preview"><div className="section-heading"><div><span className="eyebrow">处理范围</span><h3>{preview.title}</h3></div><span className="muted">{preview.parts.length} 个分 P</span></div>
        {preview.parts.length>1&&<div className="selection-tools"><button className="text-button" onClick={()=>setPages(new Set(preview.parts.map(p=>p.page)))}>选择全部 {preview.parts.length} 个分 P</button><button className="text-button" onClick={()=>setPages(new Set())}>清空选择</button></div>}
        <div className="parts-list">{preview.parts.map(part=><div className={`part-row ${pages.has(part.page)?'selected':''}`} key={part.page}>
          <label><input type="checkbox" checked={pages.has(part.page)} onChange={()=>setPages(current=>{const next=new Set(current);next.has(part.page)?next.delete(part.page):next.add(part.page);return next;})}/><span className="part-number">P{part.page}</span><span className="part-name">{part.title}</span></label>
          <span className="mono muted">{part.durationMs?timeLabel(part.durationMs):'时长待检测'}</span><span className={`status status-${part.subtitleStatus}`}>{statusLabel[part.subtitleStatus]}</span>
          {preview.sourceKind==='bilibili'&&<button className="icon-button" aria-label={`检查 P${part.page} 字幕`} disabled={!!busy} onClick={()=>void checkPart(part.page)}><RefreshCw size={14}/></button>}
        </div>)}</div>
        {preview.warnings.map((warning,i)=><p className="hint" key={i}>{warning}</p>)}
      </section>}
      <fieldset className="mode-options"><legend>处理方式</legend>{[
        ['auto','字幕优先','可读取字幕时直接提取；没有可用字幕时，只下载音轨并转写。'],
        ['subtitlesOnly','仅提取字幕','不获取音轨。需要登录或请求失败时显示具体原因。'],
        ['transcribe','重新转写音频','使用当前识别配置，生成独立文字版本。'],
      ].map(([value,title,description])=><label className={mode===value?'chosen':''} key={value}><input type="radio" name="import-mode" checked={mode===value} onChange={()=>setMode(value)}/><span><strong>{title}</strong><small>{description}</small></span></label>)}</fieldset>
    </div><footer><div>{busy?<Spinner label={busy}/>:<><strong>已选 {scope.pages.length} 项</strong><span className="muted"> · {durationLabel(scope.durationMs)}</span></>}</div><div className="actions"><button disabled={!!busy} onClick={onClose}>取消</button><button className="primary" disabled={!!busy||!preview||!scope.pages.length} onClick={()=>void submit()}>开始处理<ArrowRight size={16}/></button></div></footer>
  </Modal>;
}
