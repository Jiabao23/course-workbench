import {describe,it,expect} from 'vitest';
import {collectionTree,filterAssets,visibleCollections} from './organization';
import type {Asset,Organization} from './types';
const organization:Organization={collections:[{id:'kb',name:'学习',parentId:null},{id:'folder',name:'网络',parentId:'kb'},{id:'other',name:'工作',parentId:null}],entries:[{assetId:'a',collectionId:'folder',favorite:true},{assetId:'b',collectionId:'other',favorite:true}]};
const assets=[{id:'a',title:'网络基础'},{id:'b',title:'工作笔记'},{id:'c',title:'未分类课程'}] as Asset[];
describe('organization filters',()=>{
  it('collapsed branches hide descendants but keep other knowledge bases visible',()=>{
    const tree=collectionTree(organization.collections);
    expect(visibleCollections(tree,new Set(['kb'])).map(c=>c.id)).toEqual(['kb','other']);
    expect(visibleCollections(tree,new Set()).map(c=>c.id)).toEqual(['kb','folder','other']);
  });
  it('includes descendants and combines favorite/title filters',()=>{
    expect(filterAssets(assets,organization,{scope:'kb',favorites:true,query:'网络'}).map(a=>a.id)).toEqual(['a']);
    expect(filterAssets(assets,organization,{scope:'kb',favorites:false,query:'工作'})).toEqual([]);
    expect(filterAssets(assets,organization,{scope:'all',favorites:true,query:''}).map(a=>a.id)).toEqual(['a','b']);
  });
  it('keeps legacy courses unclassified and does not expose unrelated collections',()=>{
    expect(filterAssets(assets,organization,{scope:'unfiled',favorites:false,query:''}).map(a=>a.id)).toEqual(['c']);
    expect(filterAssets(assets,organization,{scope:'deleted',favorites:false,query:''})).toEqual([]);
  });
  it('builds stable folder paths without recursion on corrupt cycles',()=>{
    expect(collectionTree(organization.collections).find(c=>c.id==='folder')?.path).toBe('学习 / 网络');
    expect(collectionTree([{id:'x',parentId:'x',name:'cycle'}])).toEqual([]);
  });
});
