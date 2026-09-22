"""Independent CPU speech evidence worker; stdin/stdout use JSONL protocol v1.

Install silero-vad, torch, torchaudio and numpy in an isolated runtime (or add
the optional package directory to PYTHONPATH). No Whisper/CUDA initialization.
Audio is read in 512-sample windows; only the resulting speech intervals grow
with duration. Model loading is local, using Silero's bundled JIT model.
"""
import contextlib
import hashlib
import importlib.metadata
import importlib.util
import json
import math
from pathlib import Path
import sys
import wave

from worker import Protocol, PROTOCOL_VERSION, file_sha256

RATE = 16000
WINDOW = 512
DETECTOR_OPTIONS = {"backend": "jit", "device": "cpu", "threads": 1,
                    "threshold": 0.5, "sampling_rate": RATE, "window_samples": WINDOW,
                    "min_silence_duration_ms": 100, "speech_pad_ms": 30,
                    "return_seconds": False, "stream_version": 1}


def detector_identity():
    """Hash local evidence without importing Silero, torch or any model runtime."""
    spec = importlib.util.find_spec("silero_vad")
    if spec is None or not spec.submodule_search_locations:
        raise ImportError("未安装 Silero VAD")
    # Matches load_silero_vad(onnx=False) in the installed Silero package.
    model = Path(next(iter(spec.submodule_search_locations))) / "data" / "silero_vad.jit"
    if not model.is_file():
        raise ImportError("缺少 Silero VAD 本地模型 silero_vad.jit")
    directory = Path(__file__).resolve().parent
    identity = {"schema_version": 1, "python_executable": str(Path(sys.executable).resolve()),
                "python_version": sys.version, "options": DETECTOR_OPTIONS,
                "packages": {name: importlib.metadata.version(name)
                             for name in ("silero-vad", "torch", "torchaudio", "numpy")},
                "model_sha256": file_sha256(model),
                "source_sha256": {name: file_sha256(directory / name)
                                  for name in ("worker.py", "quality_worker.py")}}
    encoded = json.dumps(identity, sort_keys=True, separators=(",", ":"), allow_nan=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def validate_request(request):
    if (not isinstance(request, dict)
            or type(request.get("protocol_version")) is not int
            or request["protocol_version"] != PROTOCOL_VERSION
            or request.get("command") not in ("detect_speech", "detector_identity")):
        raise ValueError("不支持的语音检测请求")
    if not isinstance(request.get("job_id"), str) or not request["job_id"]:
        raise ValueError("缺少任务标识")
    if request["command"] == "detect_speech" and (
            not isinstance(request.get("audio_path"), str) or not request["audio_path"]):
        raise ValueError("缺少音频路径")


def load_detector():
    import numpy as np
    import torch
    from silero_vad import load_silero_vad, VADIterator

    torch.set_num_threads(DETECTOR_OPTIONS["threads"])
    model = load_silero_vad(onnx=False).to("cpu")
    model.eval()
    iterator = VADIterator(model, **{key: DETECTOR_OPTIONS[key] for key in
                                   ("threshold", "sampling_rate", "min_silence_duration_ms", "speech_pad_ms")})

    def detect(pcm):
        audio = np.frombuffer(pcm, dtype="<i2").astype(np.float32) / 32768.0
        with torch.inference_mode():
            return iterator(torch.from_numpy(audio), return_seconds=False)

    version = importlib.metadata.version("silero-vad")
    return detect, f"silero-vad-{version}:jit-cpu:stream-v1:t0.5:silence100:pad30"


def stream_speech(source, detector):
    total = source.getnframes()
    intervals, start, consumed = [], None, 0

    def append_interval(first, last):
        first, last = max(0, min(total, first)), max(0, min(total, last))
        if last <= first:
            return
        value = {"start_ms": round(first * 1000 / RATE), "end_ms": round(last * 1000 / RATE)}
        if value["end_ms"] <= value["start_ms"]:
            return
        if intervals and value["start_ms"] <= intervals[-1]["end_ms"]:
            intervals[-1]["end_ms"] = max(value["end_ms"], intervals[-1]["end_ms"])
        else:
            intervals.append(value)

    while consumed < total:
        pcm = source.readframes(min(WINDOW, total - consumed))
        if not pcm or len(pcm) % 2:
            raise ValueError("音频数据不完整")
        consumed += len(pcm) // 2
        event = detector(pcm.ljust(WINDOW * 2, b"\0"))
        if event is None:
            continue
        if not isinstance(event, dict):
            raise ValueError("语音检测返回了无效区间")
        for key in ("start", "end"):
            if key in event and (not isinstance(event[key], (int, float))
                                  or isinstance(event[key], bool) or not math.isfinite(event[key])):
                raise ValueError("语音检测返回了无效区间")
        if "start" in event:
            if start is not None:
                raise ValueError("语音检测返回了重复起点")
            start = event["start"]
        if "end" in event:
            if start is None or event["end"] < start:
                raise ValueError("语音检测返回了无效终点")
            append_interval(start, event["end"])
            start = None
    if start is not None:
        append_interval(start, total)
    return intervals


def detect_speech(request):
    path = request["audio_path"]
    before = file_sha256(path)
    with wave.open(str(path), "rb") as source:
        if (source.getnchannels() != 1 or source.getframerate() != RATE
                or source.getsampwidth() != 2 or source.getcomptype() != "NONE"
                or source.getnframes() <= 0):
            raise ValueError("语音检测需要非空单声道 16kHz PCM16 WAV")
        identity = detector_identity()
        detector, version = load_detector()
        speech = stream_speech(source, detector)
    if before != file_sha256(path):
        raise RuntimeError("检测期间音频发生变化，请重试")
    return {"speech": speech, "audio_sha256": before, "detector_version": version,
            "detector_identity": identity}


def main():
    for stream in (sys.stdin, sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(encoding="utf-8")
    protocol = Protocol(sys.stdout)
    try:
        line = sys.stdin.readline(1024 * 1024 + 1)
        if len(line) > 1024 * 1024:
            raise ValueError("worker 请求过长")
        request = json.loads(line)
        if isinstance(request, dict) and isinstance(request.get("job_id"), str):
            protocol.job_id = request["job_id"]
        validate_request(request)
        protocol.emit("hello", worker="silero-vad", version="0.1.0")
        with contextlib.redirect_stdout(sys.stderr):
            result = ({"detector_identity": detector_identity()}
                      if request["command"] == "detector_identity" else detect_speech(request))
        protocol.emit("done", **result)
        return 0
    except Exception as error:
        if isinstance(error, (ImportError, importlib.metadata.PackageNotFoundError)):
            code = "missing_dependency"
        elif isinstance(error, FileNotFoundError):
            code = "missing_audio"
        elif isinstance(error, (ValueError, wave.Error, EOFError)):
            code = "invalid_request"
        else:
            code = "worker_error"
        protocol.emit("error", code=code, message=str(error),
                      retryable=code == "worker_error")
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
