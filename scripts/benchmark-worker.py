"""Explicit actual-hardware benchmark. No synthetic transcripts or cloud API."""
import argparse
import json
from pathlib import Path
import subprocess
import time

parser = argparse.ArgumentParser()
parser.add_argument('--audio', required=True)
parser.add_argument('--python', required=True)
parser.add_argument('--ffmpeg', required=True)
parser.add_argument('--model-dir', required=True)
parser.add_argument('--output', required=True)
parser.add_argument('--model', default='small')
parser.add_argument('--device', default='cuda', choices=['cpu', 'cuda'])
parser.add_argument('--seconds', type=int)
parser.add_argument('--resume-check', action='store_true')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
output = Path(args.output).resolve()
output.mkdir(parents=True, exist_ok=True)
wave = output / 'input.wav'
if not wave.exists():
    command = [args.ffmpeg, '-hide_banner', '-loglevel', 'error', '-nostdin', '-y', '-i', args.audio,
               '-vn', '-ac', '1', '-ar', '16000', '-c:a', 'pcm_s16le']
    if args.seconds:
        command += ['-t', str(args.seconds)]
    subprocess.run(command + [str(wave)], check=True)
request = {'protocol_version': 1, 'command': 'transcribe', 'job_id': 'actual-benchmark',
           'audio_path': str(wave), 'model': args.model, 'device': args.device,
           'model_dir': args.model_dir, 'checkpoint_dir': str(output / 'checkpoints'),
           'threads': 4, 'language': 'zh', 'prompt': '', 'chunk_seconds': 300, 'allow_download': False}
reports = []
for index in range(2 if args.resume_check else 1):
    started = time.monotonic()
    with (output / f'run-{index + 1}.stderr.log').open('w', encoding='utf-8') as logs:
        process = subprocess.Popen([args.python, str(root / 'workers/asr/worker.py')],
                                   stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=logs,
                                   text=True, encoding='utf-8')
        process.stdin.write(json.dumps(request, ensure_ascii=False) + '\n')
        process.stdin.close()
        done = None
        with (output / f'run-{index + 1}.jsonl').open('w', encoding='utf-8') as protocol:
            for line in process.stdout:
                protocol.write(line)
                protocol.flush()
                message = json.loads(line)
                if message['type'] in ('progress', 'error'):
                    print(json.dumps(message, ensure_ascii=False), flush=True)
                if message['type'] == 'done':
                    done = message
        code = process.wait()
    if code != 0 or done is None:
        raise RuntimeError(f'Worker failed: exit {code}; see {output}')
    report = {**{key: value for key, value in done.items() if key != 'segments'},
              'hardware_test': 'actual', 'wall_seconds': round(time.monotonic() - started, 3),
              'segment_count': len(done['segments']), 'text_chars': sum(len(s['text']) for s in done['segments'])}
    reports.append(report)
    (output / f'run-{index + 1}.result.json').write_text(json.dumps(done, ensure_ascii=False, indent=2), encoding='utf-8')
    print(json.dumps(report, ensure_ascii=False), flush=True)
    if index == 0:
        original = done['segments']
    elif done['segments'] != original or done['resumed_chunks'] == 0:
        raise AssertionError('Resume must reproduce completed chunks without rewriting segment IDs')
(output / 'report.json').write_text(json.dumps(reports, ensure_ascii=False, indent=2), encoding='utf-8')
