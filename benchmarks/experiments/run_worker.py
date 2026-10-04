"""Run one isolated worker request; retain protocol, stderr and real process wall time."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import time
import threading


def run(python, request, output):
    output.mkdir(parents=True, exist_ok=True)
    (output/'request.json').write_text(json.dumps(request,ensure_ascii=False,indent=2),encoding='utf-8')
    flags=subprocess.CREATE_NO_WINDOW if os.name=='nt' else 0
    smi=shutil.which('nvidia-smi')
    samples=[]
    sampling_errors=[]
    stop=threading.Event()
    def sample_gpu():
        while not stop.is_set():
            try:
                value=subprocess.check_output([smi,'--id=0','--query-gpu=memory.used','--format=csv,noheader,nounits'],stderr=subprocess.STDOUT,creationflags=flags,timeout=5).decode().strip()
                samples.append(int(value))
            except Exception as error: sampling_errors.append(str(error))
            stop.wait(0.2)
    monitor=threading.Thread(target=sample_gpu,daemon=True) if smi else None
    if monitor: monitor.start()
    started=time.perf_counter()
    try:
        with (output/'protocol.jsonl').open('wb') as stdout, (output/'stderr.txt').open('wb') as stderr:
            worker=Path(__file__).resolve().parents[2]/'workers/asr/worker.py'
            child=subprocess.Popen([str(python),'-X','utf8',str(worker)],stdin=subprocess.PIPE,stdout=stdout,
                                   stderr=stderr,creationflags=flags)
            try: child.communicate((json.dumps(request,ensure_ascii=False)+'\n').encode('utf-8'),timeout=1800)
            except subprocess.TimeoutExpired:
                child.kill(); child.wait(); raise
        elapsed=time.perf_counter()-started
    finally:
        stop.set()
        if monitor: monitor.join(timeout=6)
    messages=[json.loads(line) for line in (output/'protocol.jsonl').read_text(encoding='utf-8').splitlines()]
    (output/'gpu-samples.json').write_text(json.dumps({'samples_mib':samples,'errors':sampling_errors}),encoding='utf-8')
    measurement={'process_seconds':elapsed,'exit_code':child.returncode,'python':str(python),
                 'gpu_system_peak_mib':max(samples) if samples else None,
                 'gpu_system_min_mib':min(samples) if samples else None,
                 'gpu_samples':len(samples),'gpu_sampling_ms':200,
                 'gpu_metric':'whole_device_nvidia_smi_memory_used_sampled_not_process_allocator'}
    (output/'measurement.json').write_text(json.dumps(measurement,indent=2),encoding='utf-8')
    done=next((m for m in reversed(messages) if m['type']=='done'),None)
    if child.returncode or done is None:
        print(json.dumps({'status':'failed',**measurement,'error':next((m for m in reversed(messages) if m['type']=='error'),None)},ensure_ascii=False),flush=True)
        return child.returncode or 1
    done['process_seconds']=elapsed
    done['system_gpu_measurement']=measurement
    (output/'done.json').write_text(json.dumps(done,ensure_ascii=False,indent=2),encoding='utf-8')
    print(json.dumps({'status':'done','engine':done.get('engine'),'device':done['device'],
                     'compute_type':done.get('compute_type'),'segments':len(done['segments']),
                     'timings':done.get('timings'),**measurement},ensure_ascii=False),flush=True)
    return 0


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--python',type=Path,required=True)
    parser.add_argument('--request',type=Path,required=True)
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    raise SystemExit(run(args.python,json.loads(args.request.read_text(encoding='utf-8-sig')),args.output))
