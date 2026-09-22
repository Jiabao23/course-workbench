import { describe, expect, it } from 'vitest';
import type { IntegrityIssue } from '../types';
import { contextInterval, nextPendingId, qualitySummary, bindReviewDraft, reviewDraftMatches, readerVersionAfterRefresh } from './readerQuality';

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
