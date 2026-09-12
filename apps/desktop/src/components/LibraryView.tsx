import {useEffect,useMemo,useRef,useState} from 'react';
import {BookOpen,Folder,Star,Plus,Search,FileText,Headphones,CheckCircle2,PanelLeftClose,PanelLeftOpen,FolderOpen,ArrowUpRight} from 'lucide-react';
import {api,errorMessage} from '../api';
import type {Asset,Job,Organization} from '../types';
import {collectionTree,filterAssets,defaultFilter,type LibraryFilter} from '../organization';
import {sourceLabel,timeLabel} from '../utils';
import {Empty,Modal,useConfirm} from './Common';
import './LibraryView.css';
import OrganizeDialog from './OrganizeDialog';
import CollectionNavigation from './CollectionNavigation';

type Props={setupComplete:boolean;onSettings:()=>void;assets:Asset[];jobs:Job[];organization:Organization;filter:LibraryFilter;onFilter:(filter:LibraryFilter)=>void;onOpen:(id:string)=>void;onImport:()=>void;onChanged:()=>Promise<void>;onError:(message:string)=>void;onDirty:(value:boolean)=>void};
export default function LibraryView({setupComplete,onSettings,assets,jobs,organization,filter,onFilter,onOpen,onImport,onChanged,onError,onDirty}:Props){
  const [panelCollapsed,setPanelCollapsed]=useState(()=>{try{return localStorage.getItem('course-workbench-collections-collapsed')==='true';}catch{return false;}});
  const panelToggle=useRef<HTMLButtonElement>(null),previousPanel=useRef(panelCollapsed);
  useEffect(()=>{if(previousPanel.current!==panelCollapsed){panelToggle.current?.focus();previousPanel.current=panelCollapsed;}},[panelCollapsed]);
  useEffect(()=>{try{localStorage.setItem('course-workbench-collections-collapsed',String(panelCollapsed));}catch{/* Optional layout preference. */}},[panelCollapsed]);
  const [selected,setSelected]=useState(new Set<string>()),[busy,setBusy]=useState(false);
  const [editor,setEditor]=useState<{id?:string;parentId:string|null}|null>(null),[name,setName]=useState('');
  const [moving,setMoving]=useState<string[]|null>(null),[destination,setDestination]=useState('');
  const {confirm,dialog}=useConfirm();
  const tree=useMemo(()=>collectionTree(organization.collections),[organization.collections]);
  const current=tree.find(c=>c.id===filter.scope);
  const visible=useMemo(()=>filterAssets(assets,organization,filter),[assets,organization,filter]);
  const entries=new Map(organization.entries.map(e=>[e.assetId,e]));
  useEffect(()=>{setSelected(new Set());},[filter.scope,filter.favorites,filter.query]);
  useEffect(()=>{if(filter.scope!=='all'&&filter.scope!=='unfiled'&&!organization.collections.some(c=>c.id===filter.scope))onFilter({...filter,scope:'all'});},[organization.collections,filter,onFilter]);
  useEffect(()=>{onDirty(!!editor&&!!name.trim());return()=>onDirty(false);},[editor,name,onDirty]);
  async function perform(work:()=>Promise<unknown>){setBusy(true);try{await work();await onChanged();return true;}catch(e){onError(errorMessage(e));return false;}finally{setBusy(false);}}
  function create(parentId:string|null){setName('');setEditor({parentId});}
  async function saveCollection(){if(!editor)return;const ok=await perform(()=>editor.id?api.renameCollection(editor.id,name):api.createCollection(name,editor.parentId));if(ok)setEditor(null);}
  async function removeCollection(){if(!current)return;if(!await confirm(`删除空分类“${current.name}”？含课程或子分组时会拒绝删除，课程文件与笔记不会删除。`))return;await perform(()=>api.deleteCollection(current.id));}
  function toggle(id:string){setSelected(old=>{const next=new Set(old);if(next.has(id))next.delete(id);else next.add(id);return next;});}
  function chooseScope(scope:string){onFilter({...filter,scope});}
  const title=current?.name??(filter.scope==='unfiled'?'未分类':'全部资料');
  return <section className="page library-page">
    <header className="page-heading"><div><span className="eyebrow">个人知识管理</span><h1>资料库</h1><p>按知识库整理课程，收藏随时回看的材料。</p></div><button onClick={()=>create(null)} disabled={busy}><Plus size={16}/>新建知识库</button></header>
    {!setupComplete&&<div className="first-run"><FolderOpen size={20}/><div><strong>确认你的资料与模型位置</strong><p>已检测本机环境，可在设置中调整保存目录和识别配置。</p></div><button onClick={onSettings}>查看设置<ArrowUpRight size={14}/></button></div>}
    <div className={`organization-layout ${panelCollapsed?'classification-collapsed':''}`}>
      {panelCollapsed?<div className="collection-rail"><button ref={panelToggle} className="icon-button" aria-expanded={false} aria-label="展开分类栏" title="展开分类栏" onClick={()=>setPanelCollapsed(false)}><PanelLeftOpen size={18}/></button><span>分类</span></div>:<nav className="collection-nav" aria-label="知识库分类">
      <div className="collection-panel-heading"><span>分类导航</span><button ref={panelToggle} className="icon-button" aria-expanded={true} aria-label="收起分类栏" title="收起分类栏" onClick={()=>setPanelCollapsed(true)}><PanelLeftClose size={17}/></button></div>
      <button className={filter.scope==='all'?'active':''} onClick={()=>chooseScope('all')}><BookOpen size={16}/>全部资料<span>{assets.length}</span></button>
      <button className={filter.scope==='unfiled'?'active':''} onClick={()=>chooseScope('unfiled')}><Folder size={16}/>未分类<span>{assets.filter(a=>!entries.get(a.id)?.collectionId).length}</span></button>
      <button className={filter.favorites?'active':''} aria-pressed={filter.favorites} onClick={()=>onFilter({...filter,favorites:!filter.favorites})}><Star size={16}/>仅看收藏<span>{assets.filter(a=>entries.get(a.id)?.favorite).length}</span></button>
      <div className="collection-label">我的知识库</div>
      {!tree.length&&<p className="hint">新建知识库，再把课程移入其中。</p>}
      <CollectionNavigation collections={organization.collections} scope={filter.scope} busy={busy} onSelect={chooseScope} onCreate={create} onRename={c=>{setName(c.name);setEditor({id:c.id,parentId:c.parentId});}}/>
    </nav>}<div className="collection-content">
      <div className="collection-heading"><div><h2>{title}{filter.favorites?' · 收藏':''}</h2><p className="hint">{current?.path??'分类不移动原始文件'} · {visible.length} 份{current?'（包含下级）':''}</p></div>
        {current&&<div className="actions"><button disabled={busy||current.depth>=7} onClick={()=>create(current.id)}>新建分组</button><button disabled={busy} onClick={()=>{setName(current.name);setEditor({id:current.id,parentId:current.parentId});}}>重命名</button><button disabled={busy} onClick={()=>void removeCollection()}>删除空分类</button></div>}
      </div>
      <div className="organization-tools"><label className="small-search"><Search size={16}/><input aria-label="筛选课程标题" placeholder="在当前范围筛选标题…" value={filter.query} onChange={e=>onFilter({...filter,query:e.target.value})}/></label><button disabled={busy||!selected.size} onClick={()=>{setDestination(current?.id??'');setMoving([...selected]);}}>收纳所选（{selected.size}）</button></div>
      {visible.length?<div className="organized-assets"><div className="organized-head"><input type="checkbox" aria-label="选择当前列表全部课程" checked={visible.every(a=>selected.has(a.id))} disabled={busy} onChange={e=>setSelected(e.target.checked?new Set(visible.map(a=>a.id)):new Set())}/><span>课程 / 文件</span><span>时长</span><span>状态</span><span>收藏</span><span>收纳</span></div>
        {visible.map(asset=>{const entry=entries.get(asset.id),job=jobs.find(j=>j.assetId===asset.id&&['queued','running'].includes(j.status));return <div className="organized-row" key={asset.id}>
          <input type="checkbox" aria-label={`选择课程 ${asset.title}`} checked={selected.has(asset.id)} disabled={busy} onChange={()=>toggle(asset.id)}/>
          <button className="asset-open" onClick={()=>onOpen(asset.id)}><span className="asset-icon">{asset.sourceKind==='subtitle'?<FileText size={20}/>:<Headphones size={20}/>}</span><span><strong>{asset.title}</strong><small>{sourceLabel(asset)} · {tree.find(c=>c.id===entry?.collectionId)?.path??'未分类'}</small></span></button>
          <span className="muted mono">{timeLabel(asset.durationMs)}</span><span>{job?<span className="status status-running">{job.status==='queued'?'等待':`${Math.round(job.progress)}%`}</span>:asset.activeVersionId?<span className="status status-completed"><CheckCircle2 size={12}/>可阅读</span>:<span className="status">待转写</span>}</span>
          <button className={`icon-button favorite-button ${entry?.favorite?'is-favorite':''}`} aria-label={`${entry?.favorite?'取消收藏':'收藏'} ${asset.title}`} aria-pressed={entry?.favorite??false} disabled={busy} onClick={()=>void perform(()=>api.setFavorite(asset.id,!entry?.favorite))}><Star size={18} fill={entry?.favorite?'currentColor':'none'}/></button><button className="organize-one" disabled={busy} aria-label={`收纳 ${asset.title}`} onClick={()=>{setDestination(entry?.collectionId??'');setMoving([asset.id]);}}>收纳到…</button>
        </div>;})}</div>:<Empty title={assets.length?'当前范围没有课程':'从第一条课程开始'} action={assets.length?<button onClick={()=>onFilter(defaultFilter)}>查看全部资料</button>:<button className="primary" onClick={onImport}>导入课程</button>}>{assets.length?'可以切换分类、取消收藏筛选，或把课程移动到这里。':'导入后在未分类中选择课程，再移动到知识库。'}</Empty>}
    </div></div>
    {editor&&<Modal title={editor.id?'重命名分类':editor.parentId?'新建分组':'新建知识库'} onClose={()=>{if(!busy)setEditor(null);}}><div className="modal-content"><p className="hint">{editor.parentId?`上级：${tree.find(c=>c.id===editor.parentId)?.path}`:'知识库用于整理本地课程，不会切换资料保存目录。'}</p><label className="field">名称<input autoFocus aria-label="分类名称" maxLength={80} value={name} disabled={busy} onChange={e=>setName(e.target.value)} onKeyDown={e=>{if(e.key==='Enter'&&!busy&&name.trim())void saveCollection();}}/></label><p className="hint">同一级名称不能重复；课程、文字稿与笔记保留原位置。</p></div><footer><button disabled={busy} onClick={()=>setEditor(null)}>取消</button><button className="primary" disabled={busy||!name.trim()} onClick={()=>void saveCollection()}>保存分类</button></footer></Modal>}
    {moving&&<OrganizeDialog assetIds={moving} organization={organization} initialDestination={destination} onChanged={onChanged} onClose={()=>{setMoving(null);setSelected(new Set());}} onError={onError} onDirty={onDirty}/>}{dialog}
  </section>;
}
