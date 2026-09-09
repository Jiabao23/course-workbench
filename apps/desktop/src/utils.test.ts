import { describe,it,expect } from 'vitest';
import { selectedScope,selectedText,sourceAt,citationTarget,timeLabel } from './utils';
import type { Asset,SourcePart,Transcript } from './types';
describe('explicit source and citation scope',()=>{
  it('submits only selected parts and sums their duration',()=>{
    const parts=[{page:1,durationMs:357000},{page:2,durationMs:3600000},{page:3,durationMs:60000}] as SourcePart[];
    expect(selectedScope(parts,new Set([1]))).toEqual({pages:[1],durationMs:357000});
    expect(selectedScope(parts,new Set())).toEqual({pages:[],durationMs:0});
    expect(selectedScope(parts,new Set([1,3,999]))).toEqual({pages:[1,3],durationMs:417000});
  });
  it('counts only selected original characters, including Unicode codepoints',()=>{
    const segments=[{id:'a',startMs:0,endMs:10,text:'网络🌱'},{id:'b',startMs:10,endMs:20,text:'excluded'}];
    expect(selectedText(segments,new Set(['a'])).chars).toBe(3);
    expect(selectedText(segments,new Set(['missing'])).segments).toEqual([]);
  });
  it('builds time links for the selected part without losing P number',()=>{
    const asset={bvid:'BV1JV411t7ow',page:2} as Asset;
    const url=new URL(sourceAt(asset,125999)!);
    expect(url.host).toBe('www.bilibili.com');expect(url.searchParams.get('p')).toBe('2');expect(url.searchParams.get('t')).toBe('125');
    expect(sourceAt({bvid:null} as Asset,1000)).toBeNull();
    expect(timeLabel(25*3600000+1234)).toBe('25:00:01');
  });
  it('locates citations in their bound version rather than similarly named active segments',()=>{
    const versions=[{id:'old',segments:[{id:'a',text:'old text',startMs:0,endMs:1000}]},{id:'new',segments:[{id:'a',text:'edited text',startMs:0,endMs:1000}]}] as Transcript[];
    const citation={segmentId:'a',text:'old text',startMs:0,endMs:1000};
    expect(citationTarget(versions,'old',citation)?.segment.text).toBe('old text');
    expect(citationTarget(versions,'unknown',citation)).toBeNull();
  });
});
