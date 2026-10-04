import { renderToStaticMarkup } from 'react-dom/server';
import { describe, expect, it } from 'vitest';
import CandidateCompare from './CandidateCompare';
import type { RecheckCandidate } from '../types';

describe('candidate comparison rendering',()=>{
  const candidate:RecheckCandidate={id:'c1',assetId:'a',transcriptId:'t',startMs:1000,endMs:8000,model:'large-v3',device:'cpu',originalText:'旧文字',segments:[{id:'s',startMs:1000,endMs:8000,text:'新文字'}],createdAt:'2026-01-01'};
  const props={candidates:[candidate,{...candidate,id:'c2',startMs:10000,endMs:20000}],candidate,locked:false,canAdopt:true,onCandidate:()=>{},onAdopt:()=>{},onAdoptSelected:()=>{},onDiscard:()=>{}};
  it('shows textual difference markers and starts batch selection empty',()=>{
    const html=renderToStaticMarkup(<CandidateCompare {...props}/>);
    expect(html).toContain('<del');expect(html).toContain('<ins');
    expect(html).toContain('候选文字');expect(html).toContain('采纳所选候选（0）');
    expect(html).not.toContain('checked=""');
    expect(html).toContain('未选候选仍属于旧版本');
  });
  it('does not enable adoption while there are unsaved reader changes',()=>{
    const html=renderToStaticMarkup(<CandidateCompare {...props} canAdopt={false}/>);
    expect(html).toMatch(/<button disabled="">采用并生成新版本<\/button>/);
  });
  it('offers playback for the displayed candidate and disables it without local audio',()=>{
    const html=renderToStaticMarkup(<CandidateCompare {...props} onListen={()=>{}} onLoop={()=>{}} canPlay={false}/>);
    expect(html).toMatch(/<button disabled="">回听当前候选区间<\/button>/);
    expect(html).toContain('循环回听当前候选');
    const active=renderToStaticMarkup(<CandidateCompare {...props} onListen={()=>{}} onLoop={()=>{}} canPlay looping/>);
    expect(active).toContain('停止候选循环');
  });
  it('shows persisted recognition settings without assuming metadata exists on older candidates',()=>{
    const recorded={...candidate,recognitionOptions:{engine:'faster-whisper',compute_type:'int8',beam_size:5,condition_on_previous_text:false,decoding_policy:'local-recheck-v2',engine_version:'1.2.1',model_sha256:'verified-model-hash'}};
    const html=renderToStaticMarkup(<CandidateCompare {...props} candidate={recorded}/>);
    expect(html).toContain('faster-whisper · int8');
    expect(html).toContain('不沿用前文');
    expect(html).toContain('verified-model-hash');
    expect(renderToStaticMarkup(<CandidateCompare {...props}/>)).not.toContain('识别参数与模型记录');
  });
});
