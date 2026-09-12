import type {Asset,Collection,Organization} from './types';
export type LibraryFilter={scope:string;favorites:boolean;query:string};
export const defaultFilter:LibraryFilter={scope:'all',favorites:false,query:''};
export function visibleCollections<T extends Collection>(tree:T[],collapsed:Set<string>):T[]{
  const parents=new Map(tree.map(c=>[c.id,c.parentId]));
  return tree.filter(c=>{let parent=c.parentId;const seen=new Set<string>();while(parent){if(collapsed.has(parent)||seen.has(parent))return false;seen.add(parent);parent=parents.get(parent)??null;}return true;});
}
export function collectionTree(collections:Collection[]) {
  const result:(Collection&{depth:number;path:string})[]=[],seen=new Set<string>();
  function visit(parentId:string|null,depth:number,path:string){
    for(const c of collections.filter(c=>c.parentId===parentId)){
      if(seen.has(c.id))continue;seen.add(c.id);
      const next=path?`${path} / ${c.name}`:c.name;result.push({...c,depth,path:next});visit(c.id,depth+1,next);
    }
  }
  visit(null,0,'');return result;
}
export function filterAssets(assets:Asset[],organization:Organization,filter:LibraryFilter) {
  const descendants=new Set([filter.scope]);
  for(let i=0;i<organization.collections.length;i++){
    let added=false;
    for(const c of organization.collections){if(c.parentId&&descendants.has(c.parentId)&&!descendants.has(c.id)){descendants.add(c.id);added=true;}}
    if(!added)break;
  }
  const entries=new Map(organization.entries.map(e=>[e.assetId,e]));
  const query=filter.query.trim().toLocaleLowerCase();
  return assets.filter(a=>{
    const e=entries.get(a.id);
    return (filter.scope==='all'||(filter.scope==='unfiled'?!e?.collectionId:!!e?.collectionId&&descendants.has(e.collectionId)))&&(!filter.favorites||e?.favorite)&&a.title.toLocaleLowerCase().includes(query);
  });
}
