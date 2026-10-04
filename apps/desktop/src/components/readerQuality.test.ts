import { describe, expect, it } from 'vitest';
import type { IntegrityIssue } from '../types';
import { contextInterval, nextPendingId, qualitySummary, bindReviewDraft, reviewDraftMatches, readerVersionAfterRefresh, groupIntegrityIssues, qualityCounts, batchRecheckPreview, candidateTextDiff, createAudioLoop, recheckWithRecovery, candidateSelection } from './readerQuality';

describe('reader quality behavior',()=>{
  it('keeps unresolved doubts visible independently of a legacy review',()=>{
    expect(qualitySummary({pendingCount:2,diagnosticsAvailable:true,review:{note:'checked'},audioCheck:{status:'notRun'}})).toContain('2 处待核对');
  });
  it('discloses missing recognition diagnostics even after audio detection',()=>{
    expect(qualitySummary({pendingCount:0,diagnosticsAvailable:false,audioCheck:{status:'available'}})).toContain('识别诊断缺失');
  });
  it('skips adopted revisions and informational silence when finding pending doubts',()=>{
    expect(nextPendingId([{id:'revised',severity:'warning',resolution:{status:'revised'}},{id:'silence',severity:'info',resolution:null}],'' )).toBeNull();
  });
  it('clamps context to the source and refuses oversized rechecks',()=>{
    expect(contextInterval(1000,8000,10000)).toEqual({startMs:0,endMs:10000});
    expect(contextInterval(0,120000,180000)).toBeNull();
    expect(contextInterval(5000,4000,10000)).toBeNull();
  });
  it('wraps to the next pending doubt, skipping confirmed ones',()=>{
    const issues=[{id:'a',severity:'warning',resolution:null},{id:'b',severity:'warning',resolution:{status:'confirmed'}},{id:'c',severity:'warning',resolution:{status:'pending'}}];
    expect(nextPendingId(issues,'a')).toBe('c');
    expect(nextPendingId(issues,'c')).toBe('a');
    expect(nextPendingId([issues[1]],'b')).toBeNull();
  });
});

const doubt=(id:string,startMs:number,endMs:number,code='recognitionDoubt',status?:'pending'|'confirmed'|'revised'):IntegrityIssue=>({id,startMs,endMs,code,severity:'warning',message:id,resolution:status?{status,note:'checked',reviewedAt:'2026-01-01'}:null});

describe('grouped integrity evidence',()=>{
  it('groups transitive overlaps but retains individual evidence and review states',()=>{
    const issues=[doubt('c',18000,25000,'repetition','confirmed'),doubt('a',1000,12000),doubt('b',10000,20000),doubt('adjacent',25000,30000)];
    const groups=groupIntegrityIssues(issues);
    expect(groups.map(g=>g.issues.map(i=>i.id))).toEqual([['a','b','c'],['adjacent']]);
    expect(groups[0]).toMatchObject({startMs:1000,endMs:25000,kind:'temporal'});
    expect(groups[0].issues[2].resolution?.status).toBe('confirmed');
    expect(issues[0].id).toBe('c');
    expect(nextPendingId(groups[0].issues,'a')).toBe('b');
  });
  it('separates missing evidence from suspected transcription counts',()=>{
    const issues=[doubt('a',0,20000),doubt('b',10000,30000),doubt('chunks',0,0,'unknownChunks'),doubt('duration',0,0,'unknownDuration'),doubt('incomplete',0,0,'incompleteChunks'),doubt('legacy',0,0,'noDuration')];
    expect(qualityCounts(issues)).toEqual({doubtGroups:1,pendingDoubts:2,evidencePending:4});
    expect(groupIntegrityIssues(issues).filter(g=>g.kind==='evidence')).toHaveLength(4);
    expect(qualitySummary({issues,pendingCount:6,diagnosticsAvailable:true,audioCheck:{status:'available'}})).toContain('1 处待核对');
    expect(qualitySummary({issues,pendingCount:6,diagnosticsAvailable:true,audioCheck:{status:'available'}})).toContain('4 项检查依据待补全');
  });
  it('keeps the pending group count aligned with a group containing reviewed evidence',()=>{
    const issues=[doubt('left',0,10000),doubt('bridge',5000,25000,'gap','confirmed'),doubt('right',20000,30000)];
    expect(groupIntegrityIssues(issues)).toHaveLength(1);
    expect(qualityCounts(issues)).toEqual({doubtGroups:1,pendingDoubts:2,evidencePending:0});
  });
});

describe('explicit batch recheck preview',()=>{
  it('merges overlapping pending context and expands complete boundary segments',()=>{
    const issues=[doubt('first',10000,20000),doubt('nearby',22000,30000),doubt('reviewed',50000,60000,'gap','confirmed'),doubt('missing',0,0,'unknownChunks')];
    const segments=[{id:'a',startMs:5000,endMs:12000,text:'a'},{id:'b',startMs:32000,endMs:40000,text:'b'}];
    expect(batchRecheckPreview(issues,100000,segments)).toEqual({ranges:[{startMs:5000,endMs:40000}],totalMs:35000,skipped:[]});
  });
  it('limits a preview to five ranges and 300 seconds, with skipped reasons',()=>{
    const six=Array.from({length:6},(_,i)=>doubt(String(i),10000+i*50000,20000+i*50000));
    const preview=batchRecheckPreview(six,400000,[]);
    expect(preview.ranges).toHaveLength(5);
    expect(preview.skipped).toHaveLength(1);
    expect(preview.skipped[0].reason).toContain('5');
    const long=batchRecheckPreview(Array.from({length:4},(_,i)=>doubt(String(i),10000+i*150000,110000+i*150000)),700000,[]);
    expect(long.ranges).toHaveLength(2);
    expect(long.totalMs).toBe(212000);
    expect(long.skipped.every(s=>s.reason.includes('300'))).toBe(true);
  });
  it('rejects oversized expanded ranges and requires actual audio duration',()=>{
    const issues=[doubt('long',10000,115000)];
    expect(batchRecheckPreview(issues,200000,[{id:'a',startMs:0,endMs:150000,text:'large'}]).ranges).toEqual([]);
    expect(batchRecheckPreview(issues,200000,[{id:'a',startMs:0,endMs:150000,text:'large'}]).skipped[0].reason).toContain('120');
    expect(batchRecheckPreview(issues,0,[]).ranges).toEqual([]);
  });
  it('handles unordered overlapping transcript segments without repeated full scans',()=>{
    const segments=[{id:'c',startMs:19000,endMs:30000,text:'c'},{id:'b',startMs:9000,endMs:20000,text:'b'},{id:'a',startMs:0,endMs:10000,text:'a'}];
    expect(batchRecheckPreview([doubt('x',23000,24000)],50000,segments).ranges).toEqual([{startMs:0,endMs:30000}]);
  });
  it('merges touching context boundaries just as the batch service does',()=>{
    const issues=[doubt('a',10000,20000),doubt('b',26000,30000)];
    expect(batchRecheckPreview(issues,50000,[]).ranges).toEqual([{startMs:7000,endMs:33000}]);
  });
});

describe('bounded candidate comparison',()=>{
  it('marks inserted and removed text while preserving both full strings',()=>{
    const diff=candidateTextDiff('课程讨论甲和乙。','课程讨论甲与丙。');
    expect(diff.original.map(p=>p.text).join('')).toBe('课程讨论甲和乙。');
    expect(diff.candidate.map(p=>p.text).join('')).toBe('课程讨论甲与丙。');
    expect(diff.original.filter(p=>p.changed).map(p=>p.text).join('')).toBe('和乙');
    expect(diff.candidate.filter(p=>p.changed).map(p=>p.text).join('')).toBe('与丙');
    expect(candidateTextDiff('相同🙂','相同🙂').original).toEqual([{text:'相同🙂',changed:false}]);
  });
  it('uses a coarse linear fallback for very large differences without dropping text',()=>{
    const before='开始🙂'+'甲'.repeat(30000)+'结束',after='开始🙂'+'乙'.repeat(30000)+'结束';
    const diff=candidateTextDiff(before,after);
    expect(diff.coarse).toBe(true);
    expect(diff.original.map(p=>p.text).join('')).toBe(before);
    expect(diff.candidate.map(p=>p.text).join('')).toBe(after);
    expect(diff.original).toHaveLength(3);
    expect(diff.original[0].text).toBe('开始🙂');
  });
});

describe('bounded audio loop lifecycle',()=>{
  class FakeAudio extends EventTarget {currentTime=0;ended=false;plays=0;async play(){this.plays++;}}
  it('loops only within an explicit context interval and stops enforcing it after clear',async()=>{
    const audio=new FakeAudio(),changes:Array<unknown>=[];
    const loop=createAudioLoop(audio,r=>changes.push(r));
    await loop.start({startMs:7000,endMs:23000});
    expect(audio.currentTime).toBe(7);
    expect(audio.plays).toBe(1);
    audio.currentTime=23;audio.dispatchEvent(new Event('timeupdate'));
    expect(audio.currentTime).toBe(7);
    loop.clear();audio.currentTime=24;audio.dispatchEvent(new Event('timeupdate'));
    expect(audio.currentTime).toBe(24);
    expect(changes.at(-1)).toBeNull();
  });
  it('clears on native seeking outside context and removes listeners on dispose',async()=>{
    const audio=new FakeAudio(),loop=createAudioLoop(audio,()=>{});
    await loop.start({startMs:7000,endMs:23000});
    audio.currentTime=40;audio.dispatchEvent(new Event('seeking'));audio.dispatchEvent(new Event('timeupdate'));
    expect(audio.currentTime).toBe(40);
    await loop.start({startMs:7000,endMs:23000});loop.dispose();
    audio.currentTime=24;audio.dispatchEvent(new Event('timeupdate'));
    expect(audio.currentTime).toBe(24);
  });
  it('rejects invalid or oversized loops and clears playback failures',async()=>{
    const audio=new FakeAudio(),changes:Array<unknown>=[],errors:string[]=[];
    const loop=createAudioLoop(audio,r=>changes.push(r),e=>errors.push(e));
    expect(await loop.start({startMs:0,endMs:120001})).toBe(false);
    audio.play=async()=>{throw new Error('play failed');};
    expect(await loop.start({startMs:0,endMs:10000})).toBe(false);
    expect(changes.at(-1)).toBeNull();expect(errors).toEqual(['play failed']);
  });
});

describe('partial recheck recovery',()=>{
  it('reloads persisted partial candidates after cancellation before propagating the error',async()=>{
    const restored:string[][]=[];
    await expect(recheckWithRecovery(async()=>{throw new Error('任务已取消');},async()=>['saved-first'],items=>restored.push(items))).rejects.toThrow('任务已取消');
    expect(restored).toEqual([['saved-first']]);
  });
  it('preserves the operation error and explains a failed candidate refresh',async()=>{
    await expect(recheckWithRecovery(async()=>{throw new Error('worker failed');},async()=>{throw new Error('db failed');},()=>{})).rejects.toThrow('worker failed；读取已保存候选失败：db failed');
  });
});

describe('explicit candidate adoption selection',()=>{
  const candidates=[{id:'a',startMs:0,endMs:10000},{id:'b',startMs:10000,endMs:20000},{id:'overlap',startMs:8000,endMs:12000}];
  it('adopts only selected candidates and allows touching boundaries',()=>{
    expect(candidateSelection(candidates,new Set())).toEqual({ids:[],error:''});
    expect(candidateSelection(candidates,new Set(['b','a']))).toEqual({ids:['a','b'],error:''});
  });
  it('blocks overlapping alternatives and missing candidates',()=>{
    expect(candidateSelection(candidates,new Set(['a','overlap'])).error).toContain('重叠');
    expect(candidateSelection(candidates,new Set(['removed'])).error).toContain('已变化');
  });
});

describe('draft and version stability during background refresh',()=>{
  const original:IntegrityIssue={id:'original',code:'gap',severity:'warning',startMs:0,endMs:8000,message:'original doubt',resolution:null};
  const other:IntegrityIssue={...original,id:'new-first',message:'different doubt'};
  it('binds edits to their original issue and evidence even when a refreshed report selects another issue',()=>{
    const draft=bindReviewDraft(null,original,'old-fingerprint','first reason');
    const continued=bindReviewDraft(draft,other,'new-fingerprint','continued reason');
    expect(continued?.issue).toEqual(original);
    expect(continued?.fingerprint).toBe('old-fingerprint');
    expect(continued?.text).toBe('continued reason');
    expect(reviewDraftMatches(continued,{fingerprint:'new-fingerprint',issues:[other,original]},'original')).toBe(false);
    expect(reviewDraftMatches(continued,{fingerprint:'old-fingerprint',issues:[original]},'new-first')).toBe(false);
  });
  it('blocks a removed issue and unavailable report but permits unchanged evidence',()=>{
    const draft=bindReviewDraft(null,original,'same','reason');
    expect(reviewDraftMatches(draft,null,'original')).toBe(false);
    expect(reviewDraftMatches(draft,{fingerprint:'same',issues:[other]},'original')).toBe(false);
    expect(reviewDraftMatches(draft,{fingerprint:'same',issues:[other,original]},'original')).toBe(true);
    expect(bindReviewDraft(draft,other,'new','')).toBeNull();
  });
  it('keeps the viewed version when a background job publishes a new active version',()=>{
    expect(readerVersionAfterRefresh('draft-or-history','new-active')).toBe('draft-or-history');
    expect(readerVersionAfterRefresh('','first-transcript')).toBe('first-transcript');
    expect(readerVersionAfterRefresh('',null)).toBe('');
  });
});
