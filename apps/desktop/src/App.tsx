import { useCallback,useEffect,useRef,useState } from 'react';
import { listen } from '@tauri-apps/api/event';
import { BookOpen,Library,Settings,Workflow,Plus,Search,ArrowUpRight,RefreshCw,PanelLeftClose,PanelLeftOpen,X } from 'lucide-react';
import { api,errorMessage,isDesktop } from './api';
import type { AssetDetail,Bootstrap,Job,SearchHit } from './types';
import { timeLabel } from './utils';
import { Empty,Spinner,useConfirm } from './components/Common';
import ImportDialog from './components/ImportDialog';
import JobsView from './components/JobsView';
import Reader from './components/Reader';
import SettingsPanel from './components/SettingsPanel';
import LibraryView from './components/LibraryView';
import {defaultFilter,type LibraryFilter} from './organization';
import {applyTheme} from './theme';

export default function App() {
  const [data,setData]=useState<Bootstrap|null>(null),[loading,setLoading]=useState(true),[fatal,setFatal]=useState(''),[notice,setNotice]=useState(''),[tab,setTab]=useState<'library'|'jobs'|'settings'>('library');
  const [importing,setImporting]=useState(false),[detail,setDetail]=useState<AssetDetail|null>(null),[detailLoading,setDetailLoading]=useState(false),[query,setQuery]=useState(''),[searching,setSearching]=useState(false),[hits,setHits]=useState<SearchHit[]>([]);
  const [sidebarCollapsed,setSidebarCollapsed]=useState(()=>{try{return localStorage.getItem('course-workbench-sidebar-collapsed')==='true';}catch{return false;}});
  useEffect(()=>{try{localStorage.setItem('course-workbench-sidebar-collapsed',String(sidebarCollapsed));}catch{/* Layout preferences are optional. */}},[sidebarCollapsed]);
  const [libraryFilter,setLibraryFilter]=useState<LibraryFilter>(defaultFilter);
  useEffect(()=>{setLibraryFilter(defaultFilter);},[data?.settings.dataDir]);
  useEffect(()=>{if(data)applyTheme(data.settings.theme,true);},[data?.settings.theme]);
  const [jump,setJump]=useState<{transcriptId:string;segmentId:string}|null>(null);
  const detailId=useRef<string|null>(null),detailSequence=useRef(0),refreshSequence=useRef(0),searchSequence=useRef(0),dirty=useRef(false),searchInput=useRef<HTMLInputElement>(null);
  const {confirm,dialog}=useConfirm();
  const onDirty=useCallback((value:boolean)=>{dirty.current=value;},[]);
  const notify=useCallback((message:string)=>setNotice(message),[]);
  const refresh=useCallback(async()=>{
    const id=++refreshSequence.current;
    try{const result=await api.bootstrap();if(id===refreshSequence.current){setData(result);setFatal('');}}
    catch(e){if(id===refreshSequence.current)setFatal(errorMessage(e));}
    finally{if(id===refreshSequence.current)setLoading(false);}
  },[]);
  const reloadDetail=useCallback(async()=>{
    const assetId=detailId.current;if(!assetId)return;const id=++detailSequence.current;
    try{const value=await api.assetDetail(assetId);if(detailId.current===assetId&&id===detailSequence.current)setDetail(value);}catch(e){notify(errorMessage(e));}
  },[notify]);
  const changed=useCallback(async()=>{await Promise.all([refresh(),reloadDetail()]);},[refresh,reloadDetail]);
  useEffect(()=>{if(isDesktop())void refresh();else setLoading(false);},[refresh]);
  useEffect(()=>{
    if(!isDesktop())return;let disposed=false;let unsubscribe:(()=>void)|undefined;
    void listen<Job>('job-updated',event=>{
      const job=event.payload;
      setData(current=>current?{...current,jobs:[job,...current.jobs.filter(j=>j.id!==job.id)].sort((a,b)=>b.createdAt.localeCompare(a.createdAt))}:current);
      if(['completed','failed','cancelled'].includes(job.status)){void refresh();if(detailId.current===job.assetId)void reloadDetail();}
    }).then(stop=>{if(disposed)stop();else unsubscribe=stop;}).catch(e=>notify(errorMessage(e)));
    return()=>{disposed=true;unsubscribe?.();};
  },[refresh,reloadDetail,notify]);
  const running=data?.jobs.some(j=>j.status==='running'||j.status==='queued')??false;
  useEffect(()=>{if(!running)return;const timer=setInterval(()=>void refresh(),6000);return()=>clearInterval(timer);},[running,refresh]);
  useEffect(()=>{const listener=(e:KeyboardEvent)=>{if((e.ctrlKey||e.metaKey)&&e.key.toLowerCase()==='k'){e.preventDefault();searchInput.current?.focus();}};window.addEventListener('keydown',listener);return()=>window.removeEventListener('keydown',listener);},[]);
  useEffect(()=>{
    const id=++searchSequence.current;if(!query.trim()){setHits([]);setSearching(false);return;}
    setSearching(true);const timer=setTimeout(()=>{void api.search(query.trim()).then(result=>{if(id===searchSequence.current)setHits(result);}).catch(e=>{if(id===searchSequence.current)notify(errorMessage(e));}).finally(()=>{if(id===searchSequence.current)setSearching(false);});},250);
    return()=>clearTimeout(timer);
  },[query,notify]);
  useEffect(()=>{if(!notice)return;const timer=setTimeout(()=>setNotice(''),notice.startsWith('已')?7000:18000);return()=>clearTimeout(timer);},[notice]);
  async function guard(action:()=>void){if(dirty.current&&!(await confirm('有尚未保存的修改。继续后会放弃这些修改。')))return;dirty.current=false;action();}
  function closeReader(){detailSequence.current++;detailId.current=null;setDetail(null);setJump(null);setDetailLoading(false);}
  async function navigate(next:'library'|'jobs'|'settings'){await guard(()=>{closeReader();setQuery('');setTab(next);});}
  async function openAsset(assetId:string,target:typeof jump=null){await guard(()=>{
    const id=++detailSequence.current;detailId.current=assetId;setTab('library');setDetail(null);setDetailLoading(true);setQuery('');setJump(target);
    void api.assetDetail(assetId).then(result=>{if(detailSequence.current===id)setDetail(result);}).catch(e=>notify(errorMessage(e))).finally(()=>{if(detailSequence.current===id)setDetailLoading(false);});
  });}
  async function settingsSaved(){closeReader();await refresh();}
  const count=data?.jobs.filter(j=>['running','queued','paused'].includes(j.status)).length??0;
  if(!isDesktop())return <div className="desktop-required"><BookOpen size={40}/><h1>课程工作台</h1><p>请打开 Windows 桌面应用，连接本地资料库、识别环境和文件。</p><p className="hint">此页面是界面开发入口，不包含模拟资料。</p></div>;
  return <div className="app-shell"><aside className={`sidebar ${sidebarCollapsed?'is-collapsed':''}`}><div className="brand"><div className="brand-mark"><BookOpen size={23}/></div><div><strong>课程工作台</strong><span>个人知识资料库</span></div></div><button className="primary import-button" aria-label="导入课程" title="导入课程" disabled={!data} onClick={()=>setImporting(true)}><Plus size={18}/><span>导入课程</span></button><nav aria-label="主导航">{[
    ['library','资料库',Library],['jobs','任务',Workflow],['settings','设置',Settings],
  ].map(([id,label,Icon])=><button key={String(id)} aria-label={String(label)} title={String(label)} className={tab===id?'active':''} onClick={()=>void navigate(id as typeof tab)}>{typeof Icon!=='string'&&<Icon size={19}/>}<span>{String(label)}</span>{id==='jobs'&&count>0&&<span className="nav-count">{count}</span>}</button>)}</nav><div className="sidebar-bottom"><div className="local-indicator"><span/>资料保存在本地</div>{data&&<><p title={data.resources.gpu?.name??data.resources.cpuName}>{data.resources.cudaAvailable?data.resources.gpu?.name.replace('NVIDIA GeForce ','').replace(' Laptop GPU',''):'CPU 识别路径'}</p><button className="text-button" onClick={()=>void navigate('settings')}>{data.settings.preset==='eco'?'省资源':data.settings.preset==='quality'?'高质量':data.settings.preset==='custom'?'自定义':'均衡'}配置<ArrowUpRight size={12}/></button></>}</div></aside>
    <div className="workspace"><header className="app-toolbar"><button className="icon-button" aria-expanded={!sidebarCollapsed} aria-label={sidebarCollapsed?'展开侧栏':'收起侧栏'} title={sidebarCollapsed?'展开侧栏':'收起侧栏'} onClick={()=>setSidebarCollapsed(value=>!value)}>{sidebarCollapsed?<PanelLeftOpen size={18}/>:<PanelLeftClose size={18}/>}</button><div className="breadcrumb">我的课程<span>/</span>{detail?'阅读与笔记':tab==='library'?'资料库':tab==='jobs'?'任务管理':'偏好设置'}</div><div className="global-search"><Search size={16}/><input ref={searchInput} aria-label="搜索资料库" placeholder="搜索原文与课程…" value={query} onChange={e=>{const value=e.target.value;void guard(()=>setQuery(value));}}/><kbd>Ctrl K</kbd>{query&&<button className="icon-button" aria-label="清空搜索" onClick={()=>setQuery('')}><X size={14}/></button>}</div><button className="icon-button" aria-label="刷新资料库" title="刷新资料库" disabled={loading} onClick={()=>void changed()}><RefreshCw size={17}/></button></header>
      {loading?<div className="loading-screen"><Spinner label="正在读取本地资源与资料库…"/><p className="hint">首次检测识别环境需要几秒钟。</p></div>:fatal&&!data?<div className="page"><Empty title="暂时无法打开资料库" action={<button className="primary" onClick={()=>{setLoading(true);void refresh();}}>重新连接</button>}>{fatal}</Empty></div>:data?<>
        {query.trim()?<section className="page"><header className="page-heading"><div><span className="eyebrow">本地全文搜索</span><h1>搜索结果</h1><p>{searching?'正在查找…':`找到 ${hits.length} 处相关原文`}</p></div></header>{searching?<Spinner label="检索中"/>:hits.length?<div className="search-results">{hits.map(hit=><button key={`${hit.transcriptId}-${hit.segmentId}`} onClick={()=>void openAsset(hit.assetId,{transcriptId:hit.transcriptId,segmentId:hit.segmentId})}><span className="search-result-title">{hit.assetTitle}<span className="mono">{timeLabel(hit.startMs)}</span></span><p>{hit.text}</p><span className="text-action">定位原文<ArrowUpRight size={13}/></span></button>)}</div>:<Empty title="没有找到相关原文">可以尝试更短的词语，或先完成课程文字稿。</Empty>}</section>
        :detailLoading?<div className="loading-screen"><Spinner label="打开课程…"/></div>:detail?<Reader key={detail.asset.id} detail={detail} settings={data.settings} apiKeyConfigured={data.apiKeyConfigured} onBack={()=>void navigate('library')} onChanged={changed} onSettings={()=>void navigate('settings')} onDirty={onDirty} onError={notify} initialJump={jump}/>
        :tab==='jobs'?<JobsView jobs={data.jobs} onRefresh={refresh} onOpen={id=>void openAsset(id)} onError={notify}/>
        :tab==='settings'?<SettingsPanel settings={data.settings} assets={data.assets} resources={data.resources} recommendation={data.recommendation} apiKeyConfigured={data.apiKeyConfigured} onDirty={onDirty} onSaved={settingsSaved} onError={notify}/>
        :<LibraryView setupComplete={data.settings.setupComplete} onSettings={()=>void navigate('settings')} key={data.settings.dataDir} assets={data.assets} jobs={data.jobs} organization={data.organization} filter={libraryFilter} onFilter={setLibraryFilter} onOpen={id=>void openAsset(id)} onImport={()=>setImporting(true)} onChanged={refresh} onError={notify} onDirty={onDirty}/>}
      </>:null}
    </div>{notice&&<div className={`toast ${notice.startsWith('已')?'success':''}`} role="status"><span>{notice}</span><button className="icon-button" aria-label="关闭提示" onClick={()=>setNotice('')}><X size={17}/></button></div>}{importing&&<ImportDialog onClose={()=>setImporting(false)} onImported={async()=>{await refresh();await navigate('jobs');}}/>}{dialog}
  </div>;
}
