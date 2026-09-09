"""Kill an actual ASR worker after one checkpoint, then verify partial resume.

Uses a user-supplied audio/model, never downloads a model, and writes only to a
new output directory. This is a hardware experiment, separate from unit tests.
"""
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
parser.add_argument('--device', choices=['cpu', 'cuda'], default='cuda')
parser.add_argument('--model', default='small')
args = parser.parse_args()
root = Path(__file__).resolve().parents[1]
output = Path(args.output).resolve()
output.mkdir(parents=True, exist_ok=False)
audio = output / 'sample.wav'
subprocess.run([args.ffmpeg, '-hide_banner', '-loglevel', 'error', '-nostdin',
                '-i', args.audio, '-t', '70', '-vn', '-ac', '1', '-ar', '16000',
                '-c:a', 'pcm_s16le', str(audio)], check=True)
request = dict(protocol_version=1, command='transcribe', job_id='recovery-test',
               audio_path=str(audio), checkpoint_dir=str(output / 'checkpoints'),
               model_dir=args.model_dir, model=args.model, device=args.device,
               language='zh', threads=4, prompt='', chunk_seconds=30, allow_download=False)


def run(label, interrupt=False):
    started = time.monotonic()
    done = None
    with (output / f'{label}.stderr.log').open('w', encoding='utf-8') as logs:
        child = subprocess.Popen([args.python, str(root / 'workers/asr/worker.py')],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                                 stderr=logs, text=True, encoding='utf-8')
        try:
            child.stdin.write(json.dumps(request) + '\n')
            child.stdin.close()
            with (output / f'{label}.jsonl').open('w', encoding='utf-8') as protocol:
                for line in child.stdout:
                    message = json.loads(line)
                    protocol.write(line)
                    protocol.flush()
                    if message['type'] in ('progress', 'checkpoint', 'error'):
                        print(json.dumps(message, ensure_ascii=False), flush=True)
                    if interrupt and message['type'] == 'checkpoint':
                        child.kill()
                        child.wait(timeout=15)
                        return {'killed_after_chunks': message['chunk_done']}
                    if message['type'] == 'done':
                        done = message
            assert child.wait(timeout=15) == 0 and done, 'Worker did not complete'
            done['wall_seconds'] = round(time.monotonic() - started, 3)
            return done
        finally:
            if child.poll() is None:
                child.kill()
                child.wait(timeout=15)


interrupted = run('interrupted', True)
assert interrupted['killed_after_chunks'] == 1
checkpoint = next((output / 'checkpoints').glob('*/000000.json'))
before = checkpoint.read_bytes()
first = json.loads(before)['segments']
resumed = run('resumed')
assert resumed['resumed_chunks'] == 1, 'Expected exactly the completed first chunk'
assert checkpoint.read_bytes() == before, 'A completed chunk was rewritten'
assert resumed['segments'][:len(first)] == first, 'Completed segment IDs/text changed'
ids = [segment['id'] for segment in resumed['segments']]
assert len(ids) == len(set(ids)), 'Duplicate segments after recovery'
replayed = run('replayed')
assert replayed['segments'] == resumed['segments'] and replayed['resumed_chunks'] == 3
report = dict(hardware_test='actual', model=args.model, device=args.device,
              audio_seconds=70, chunk_seconds=30, killed_after_chunks=1,
              resumed_chunks=resumed['resumed_chunks'],
              replayed_chunks=replayed['resumed_chunks'],
              segment_count=len(ids), checkpoint_unchanged=True, duplicate_segments=0,
              resume_wall_seconds=resumed['wall_seconds'],
              replay_wall_seconds=replayed['wall_seconds'])
(output / 'report.json').write_text(json.dumps(report, indent=2), encoding='utf-8')
print(json.dumps(report), flush=True)
