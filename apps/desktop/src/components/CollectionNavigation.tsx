import {useEffect,useRef,useState} from 'react';
import {BookOpen,Folder,ChevronRight,MoreHorizontal} from 'lucide-react';
import type {Collection} from '../types';
import {collectionTree,visibleCollections} from '../organization';

type Props={collections:Collection[];scope:string;busy:boolean;onSelect:(id:string)=>void;onCreate:(parentId:string)=>void;onRename:(collection:Collection)=>void};
export default function CollectionNavigation({collections,scope,busy,onSelect,onCreate,onRename}:Props){
  const [collapsed,setCollapsed]=useState(new Set<string>()),[menu,setMenu]=useState<string|null>(null);
  const popup=useRef<HTMLDivElement>(null);
  const tree=collectionTree(collections);
  useEffect(()=>{if(!menu)return;function outside(e:PointerEvent){if(!popup.current?.contains(e.target as Node))setMenu(null);}function escape(e:KeyboardEvent){if(e.key==='Escape')setMenu(null);}document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);};},[menu]);
  function fold(id:string){setCollapsed(old=>{const next=new Set(old);if(next.has(id))next.delete(id);else next.add(id);return next;});}
  return <>{visibleCollections(tree,collapsed).map(c=><div key={c.id} className="collection-branch" style={{paddingLeft:c.depth*9}}>
    {tree.some(child=>child.parentId===c.id)?<button className="icon-button" disabled={busy} aria-label={`${collapsed.has(c.id)?'展开左侧分支':'收起左侧分支'} ${c.path}`} aria-expanded={!collapsed.has(c.id)} onClick={()=>fold(c.id)}><ChevronRight size={13} style={{transform:collapsed.has(c.id)?'rotate(0deg)':'rotate(90deg)'}}/></button>:<span/>}
    <button className={`collection-select ${scope===c.id?'active':''}`} title={c.path} aria-label={`查看分类 ${c.path}`} onClick={()=>{setMenu(null);onSelect(c.id);}}>{c.parentId?<Folder size={14}/>:<BookOpen size={14}/>}<span>{c.name}</span></button>
    <div className="branch-options" ref={menu===c.id?popup:null}><button className="icon-button" disabled={busy} aria-label={`分支操作 ${c.path}`} aria-expanded={menu===c.id} onClick={()=>setMenu(old=>old===c.id?null:c.id)}><MoreHorizontal size={15}/></button>
      {menu===c.id&&<div className="branch-popup" aria-label={`${c.name}的操作`}><strong>{c.name}</strong><button disabled={c.depth>=7} onClick={()=>{setMenu(null);setCollapsed(new Set());onCreate(c.id);}}>新建子分组</button><button onClick={()=>{setMenu(null);onRename(c);}}>重命名</button><button onClick={()=>{setMenu(null);onSelect(c.id);}}>查看此分类</button></div>}
    </div>
  </div>)}</>;
}
