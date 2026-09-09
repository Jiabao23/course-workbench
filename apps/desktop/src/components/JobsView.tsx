import { RotateCcw,XCircle,ArrowUpRight,CheckCircle2,Clock3 } from 'lucide-react';
import type { Job } from '../types';
import { api,errorMessage } from '../api';
import { Empty } from './Common';
export default function JobsView({jobs,onRefresh,onOpen,onError}:{jobs:Job[];onRefresh:()=>Promise<void>;onOpen:(id:string)=>void;onError:(error:string)=>void}) {
  async function action(job:Job,kind:string){try{if(kind==='cancel')await api.cancelJob(job.id);else await api.retryJob(job.id,kind==='current');await onRefresh();}catch(e){onError(errorMessage(e));}}
  const labels={queued:'等待中',running:'处理中',paused:'待恢复',completed:'已完成',failed:'失败',cancelled:'已取消'};
  return <section className="page jobs-page"><header className="page-heading"><div><span className="eyebrow">处理记录</span><h1>任务</h1><p>关闭应用后，已完成的转写分块会保留。</p></div><span className="muted">{jobs.filter(j=>['queued','running'].includes(j.status)).length} 项待完成</span></header>
    {!jobs.length?<Empty title="还没有处理任务">导入一条课程后，可以在这里查看进度。</Empty>:<div className="jobs-list">{jobs.map(job=><article className="job-row" key={job.id}>
      <div className={`job-state ${job.status}`}>{job.status==='completed'?<CheckCircle2 size={22}/>:job.status==='failed'?<XCircle size={22}/>:<Clock3 size={22}/>}</div>
      <div className="job-body"><div className="job-title"><button className="text-button" onClick={()=>onOpen(job.assetId)}>{job.title}<ArrowUpRight size={14}/></button><span className={`status status-${job.status}`}>{labels[job.status]}</span></div>
        <div className="progress-track" role="progressbar" aria-label="任务进度" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(job.progress)}><span style={{width:`${job.progress}%`}}/></div>
        <div className="job-meta"><span>{job.stage}</span><span>{Math.round(job.progress)}% · {job.model} / {job.device.toUpperCase()}{job.chunkTotal?` · ${job.chunkDone}/${job.chunkTotal} 分块`:''}</span></div>
        {job.error&&<details className="job-error" open={job.status==='failed'}><summary>查看原因</summary><pre>{job.error}</pre></details>}
      </div><div className="job-actions">{['queued','running'].includes(job.status)&&<button onClick={()=>void action(job,'cancel')}><XCircle size={15}/>取消</button>}{['paused','failed','cancelled'].includes(job.status)&&<><button onClick={()=>void action(job,'retry')}><RotateCcw size={14}/>按原配置继续</button><button className="text-button" onClick={()=>void action(job,'current')}>按当前配置重试</button></>}</div>
    </article>)}</div>}
  </section>;
}
