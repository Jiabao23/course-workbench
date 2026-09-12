import {useEffect,useState} from 'react';
import {BookOpen,Folder,Plus,ChevronRight,Check} from 'lucide-react';
import type {Organization} from '../types';
import {collectionTree,visibleCollections} from '../organization';
import {api,errorMessage} from '../api';
import {Modal,Spinner,useConfirm} from './Common';

type Props={assetIds:string[];organization:Organization;initialDestination:string;onChanged:()=>Promise<void>;onClose:()=>void;onError:(message:string)=>void;onDirty:(value:boolean)=>void};
export default function OrganizeDialog({assetIds,organization,initialDestination,onChanged,onClose,onError,onDirty}:Props){
  const [destination,setDestination]=useState(initialDestination),[busy,setBusy]=useState(false);
  const [collapsed,setCollapsed]=useState(new Set<string>());
  const [branch,setBranch]=useState<{parentId:string|null;name:string}|null>(null);
  const {confirm,dialog}=useConfirm();
  const tree=collectionTree(organization.collections),target=tree.find(c=>c.id===destination);
  useEffect(()=>{onDirty(busy||!!branch?.name.trim());return()=>onDirty(false);},[busy,branch?.name,onDirty]);
  async function close(){if(busy)return;if(branch?.name.trim()&&!await confirm('新分组名称尚未保存，放弃填写并关闭面板？'))return;onClose();}
  async function create(){if(!branch)return;setBusy(true);try{const result=await api.createCollection(branch.name,branch.parentId);await onChanged();setDestination(result.id);setCollapsed(new Set());setBranch(null);}catch(e){onError(errorMessage(e));}finally{setBusy(false);}}
  async function move(){setBusy(true);try{await api.moveAssets(assetIds,destination||null);await onChanged();onClose();}catch(e){onError(errorMessage(e));}finally{setBusy(false);}}
  function fold(id:string){setCollapsed(old=>{const next=new Set(old);if(next.has(id))next.delete(id);else next.add(id);return next;});}
  return <><Modal title="收纳到…" onClose={()=>void close()}>
    <div className="modal-content organize-dialog"><p>将 {assetIds.length} 份课程收纳到选定位置。收藏、原文和历史笔记保留。</p>
      <div className="destination-tree" aria-label="收纳位置">
        <button className={!destination?'chosen':''} aria-pressed={!destination} disabled={busy} onClick={()=>setDestination('')}><Folder size={16}/><span>未分类</span>{!destination&&<Check size={15}/>}</button>
        {visibleCollections(tree,collapsed).map(c=><div key={c.id} className="destination-branch" style={{paddingLeft:c.depth*16}}>
          {tree.some(child=>child.parentId===c.id)?<button className="icon-button branch-toggle" disabled={busy} aria-label={`${collapsed.has(c.id)?'展开':'折叠'} ${c.path}`} aria-expanded={!collapsed.has(c.id)} onClick={()=>fold(c.id)}><ChevronRight size={14} style={{transform:collapsed.has(c.id)?'rotate(0deg)':'rotate(90deg)'}}/></button>:<span className="branch-spacer"/>}
          <button className={destination===c.id?'chosen':''} disabled={busy} title={c.path} aria-label={`收纳目标 ${c.path}`} aria-pressed={destination===c.id} onClick={()=>setDestination(c.id)}>{c.parentId?<Folder size={16}/>:<BookOpen size={16}/>}<span>{c.name}</span>{destination===c.id&&<Check size={15}/>}</button>
        </div>)}
      </div>
      <p className="destination-summary">目标：{target?.path??(destination?'分类已失效，请重新选择':'未分类')}</p>
      <div className="actions"><button disabled={busy||!!branch} onClick={()=>setBranch({parentId:null,name:''})}><Plus size={14}/>新建知识库</button><button disabled={busy||!!branch||!target||target.depth>=7} onClick={()=>setBranch({parentId:destination,name:''})}><Plus size={14}/>新建子分组</button></div>
      {branch&&<div className="inline-branch"><label className="field">{branch.parentId?`在“${tree.find(c=>c.id===branch.parentId)?.path}”下新建`:'新建知识库'}<input autoFocus aria-label="新分支名称" maxLength={80} disabled={busy} value={branch.name} onChange={e=>setBranch({...branch,name:e.target.value})} onKeyDown={e=>{if(e.key==='Enter'&&branch.name.trim()&&!busy)void create();}}/></label><div className="actions"><button disabled={busy} onClick={()=>setBranch(null)}>取消新建</button><button disabled={busy||!branch.name.trim()} onClick={()=>void create()}>创建并选中</button></div></div>}
      <p className="hint">新建的分类会立即保存；点击“确认收纳”后才移动课程。</p>
      {busy&&<Spinner label="正在保存收纳位置…"/>}
    </div><footer><button disabled={busy} onClick={()=>void close()}>取消</button><button className="primary" disabled={busy||!!branch||!!destination&&!target} onClick={()=>void move()}>确认收纳</button></footer>
  </Modal>{dialog}</>;
}
