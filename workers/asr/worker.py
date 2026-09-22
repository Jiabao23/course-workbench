"""Whisper worker. stdout is JSONL v1; diagnostics go to stderr.

Heavy dependencies are deliberately lazy: protocol and checkpoint tests need
only Python's standard library. Input audio is mono 16 kHz PCM16 WAV, prepared
by the desktop process. At most one chunk is decoded into RAM at a time.
"""

import contextlib
import hashlib
import importlib.util
import importlib.metadata
import json
import math
import os
from pathlib import Path
import platform
import sys
import tempfile
import time
import uuid
import wave

PROTOCOL_VERSION = 1
MODELS = ("tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium",
          "medium.en", "large", "large-v1", "large-v2", "large-v3", "turbo",
          "large-v3-turbo")
IDENTITY_FIELDS = ("model", "device", "language", "prompt", "chunk_seconds",
                   "threads", "engine_version", "model_sha256")


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def checkpoint_key(audio_path, options):
    identity = {key: options.get(key) for key in IDENTITY_FIELDS}
    identity.update(protocol_version=PROTOCOL_VERSION,
                    audio_sha256=file_sha256(audio_path), normalizer_version=1)
    encoded = json.dumps(identity, sort_keys=True, ensure_ascii=False).encode("utf-8")
    return hashlib.sha256(encoded).hexdigest()


def plan_chunks(duration_seconds, chunk_seconds=300):
    if not math.isfinite(duration_seconds) or duration_seconds <= 0:
        raise ValueError("音频时长必须大于 0")
    if not math.isfinite(chunk_seconds) or chunk_seconds <= 0:
        raise ValueError("分块时长必须大于 0")
    return [(float(i * chunk_seconds), min(float((i + 1) * chunk_seconds), duration_seconds))
            for i in range(math.ceil(duration_seconds / chunk_seconds))]


def normalize_segments(segments, offset_ms, duration_ms, prefix):
    result = []
    for index, item in enumerate(segments):
        text = str(item.get("text", "")).strip()
        start, end = float(item["start"]), float(item["end"])
        if not text or not math.isfinite(start) or not math.isfinite(end):
            continue
        start_ms = max(offset_ms, min(duration_ms, offset_ms + round(start * 1000)))
        end_ms = max(start_ms, min(duration_ms, offset_ms + round(end * 1000)))
        if end_ms <= start_ms:
            continue
        segment = {"id": str(uuid.uuid5(uuid.NAMESPACE_URL, f"cw:{prefix}:{index}")),
                   "start_ms": start_ms, "end_ms": end_ms, "text": text}
        diagnostics = {key: item[key] for key in
                       ("avg_logprob", "no_speech_prob", "compression_ratio")
                       if isinstance(item.get(key), (int, float))
                       and not isinstance(item[key], bool) and math.isfinite(item[key])}
        if diagnostics:
            segment["diagnostics"] = diagnostics
        result.append(segment)
    return sorted(result, key=lambda item: item["start_ms"])


def validate_request(request):
    if not isinstance(request, dict) or request.get("protocol_version") != PROTOCOL_VERSION:
        raise ValueError("不支持的 worker 协议版本")
    if request.get("command") not in ("probe", "transcribe", "download_model"):
        raise ValueError("未知 worker 命令")
    if request["command"] == "probe":
        return
    if request.get("model") not in MODELS:
        raise ValueError("请选择支持的 Whisper 模型")
    if not request.get("model_dir"):
        raise ValueError("请配置模型目录")
    if request["command"] == "download_model":
        return
    if request.get("device") not in ("cpu", "cuda"):
        raise ValueError("首版仅支持 cpu 或 cuda 设备")
    if not isinstance(request.get("threads", 4), int) or not 1 <= request.get("threads", 4) <= 256:
        raise ValueError("线程数必须为 1 到 256")
    chunk = request.get("chunk_seconds", 300)
    if not isinstance(chunk, (int, float)) or not math.isfinite(chunk) or not 30 <= chunk <= 1800:
        raise ValueError("分块时长必须为 30 到 1800 秒")
    if not request.get("audio_path") or not request.get("checkpoint_dir"):
        raise ValueError("缺少音频或检查点目录")
    if not isinstance(request.get("prompt", ""), str) or len(request.get("prompt", "")) > 4000:
        raise ValueError("术语提示应少于 4000 字符")
    if not isinstance(request.get("allow_download", False), bool):
        raise ValueError("allow_download 必须为布尔值")


class Protocol:
    def __init__(self, stream, job_id=""):
        self.stream, self.job_id = stream, job_id

    def emit(self, kind, **fields):
        value = {"protocol_version": PROTOCOL_VERSION, "job_id": self.job_id,
                 "type": kind, **fields}
        self.stream.write(json.dumps(value, ensure_ascii=False, allow_nan=False) + "\n")
        self.stream.flush()


def atomic_json(path, value):
    """A killed worker leaves either the prior checkpoint or a complete new one."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(mode="w", encoding="utf-8", dir=path.parent,
                                         prefix=".chunk-", suffix=".tmp", delete=False) as stream:
            temporary = stream.name
            json.dump(value, stream, ensure_ascii=False, allow_nan=False)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if temporary and os.path.exists(temporary):
            os.unlink(temporary)


def read_checkpoint(path, key, index, start_ms, end_ms):
    try:
        item = json.loads(Path(path).read_text(encoding="utf-8"))
        if (item.get("protocol_version") != 1 or item.get("key") != key
                or item.get("index") != index or item.get("start_ms") != start_ms
                or item.get("end_ms") != end_ms or not isinstance(item.get("segments"), list)):
            return None
        previous, identifiers = start_ms, set()
        for segment in item["segments"]:
            if "diagnostics" in segment:
                diagnostics = segment["diagnostics"]
                if (not isinstance(diagnostics, dict)
                        or any(not isinstance(value, (int, float)) or isinstance(value, bool)
                               or not math.isfinite(value) for value in diagnostics.values())):
                    return None
            if (not isinstance(segment.get("text"), str) or not segment["text"].strip()
                    or not isinstance(segment.get("id"), str) or segment["id"] in identifiers
                    or not isinstance(segment.get("start_ms"), int)
                    or not isinstance(segment.get("end_ms"), int)
                    or not previous <= segment["start_ms"] < segment["end_ms"] <= end_ms):
                return None
            previous = segment["start_ms"]
            identifiers.add(segment["id"])
        return item["segments"]
    except (OSError, ValueError, TypeError, AttributeError):
        return None


def peak_ram_mb():
    if sys.platform == "win32":
        import ctypes
        from ctypes import wintypes

        class Counters(ctypes.Structure):
            _fields_ = [("cb", wintypes.DWORD), ("PageFaultCount", wintypes.DWORD)] + [
                (name, ctypes.c_size_t) for name in ("PeakWorkingSetSize", "WorkingSetSize",
                "QuotaPeakPagedPoolUsage", "QuotaPagedPoolUsage", "QuotaPeakNonPagedPoolUsage",
                "QuotaNonPagedPoolUsage", "PagefileUsage", "PeakPagefileUsage")]

        counters = Counters()
        counters.cb = ctypes.sizeof(counters)
        kernel = ctypes.WinDLL("kernel32", use_last_error=True)
        kernel.GetCurrentProcess.restype = wintypes.HANDLE
        psapi = ctypes.WinDLL("psapi", use_last_error=True)
        psapi.GetProcessMemoryInfo.argtypes = [wintypes.HANDLE, ctypes.POINTER(Counters), wintypes.DWORD]
        if psapi.GetProcessMemoryInfo(kernel.GetCurrentProcess(), ctypes.byref(counters), counters.cb):
            return round(counters.PeakWorkingSetSize / 1024 ** 2, 1)
        return None
    try:
        import resource
        peak = resource.getrusage(resource.RUSAGE_SELF).ru_maxrss
        return round(peak / (1024 ** 2 if sys.platform == "darwin" else 1024), 1)
    except ImportError:
        return None


def probe(request):
    dependencies = {name: importlib.util.find_spec(name) is not None
                    for name in ("torch", "whisper", "numpy", "yt_dlp")}
    info = {"python_version": platform.python_version(), "executable": sys.executable,
            "dependencies": dependencies, "torch_version": None, "cuda_version": None,
            "cuda_available": False, "gpu_name": None, "gpu_total_mb": 0, "gpu_free_mb": 0,
            "models": [], "warnings": []}
    try:
        info["engine_version"] = importlib.metadata.version("openai-whisper")
    except importlib.metadata.PackageNotFoundError:
        info["engine_version"] = None
    if dependencies["torch"]:
        try:
            import torch
            info.update(torch_version=str(torch.__version__), cuda_version=torch.version.cuda,
                        cuda_available=torch.cuda.is_available())
            if info["cuda_available"]:
                free, total = torch.cuda.mem_get_info()
                info.update(gpu_name=torch.cuda.get_device_name(0),
                            gpu_total_mb=round(total / 1024 ** 2), gpu_free_mb=round(free / 1024 ** 2))
        except Exception as error:
            info["warnings"].append(str(error))
    directory = Path(request.get("model_dir", "."))
    if directory.is_dir():
        for name in MODELS:
            path = directory / f"{name}.pt"
            if path.is_file():
                info["models"].append({"name": name, "size_bytes": path.stat().st_size})
    return info


def ensure_model(request, protocol):
    import whisper
    model_name = request["model"]
    if model_name not in whisper.available_models():
        raise ValueError(f"当前 Whisper 版本不支持模型 {model_name}")
    directory = Path(request["model_dir"]).expanduser().resolve()
    directory.mkdir(parents=True, exist_ok=True)
    url = whisper._MODELS[model_name]
    filename = url.rsplit("/", 1)[-1]
    expected_sha256 = url.split("/")[-2]
    path = directory / filename
    if path.is_file() and file_sha256(path) == expected_sha256:
        return path, expected_sha256
    if not request.get("allow_download", False) and request["command"] != "download_model":
        error = "模型校验失败" if path.exists() else "模型尚未下载"
        raise FileNotFoundError(f"{error}：{model_name}。请在设置中下载或选择已有模型目录。")
    protocol.emit("progress", progress=0, stage="download_model", model=model_name)
    # Whisper's official downloader validates SHA256 and writes logs to stderr.
    whisper._download(url, str(directory), in_memory=False)
    if not path.is_file() or file_sha256(path) != expected_sha256:
        raise RuntimeError("下载的模型未通过 SHA256 校验，请重试")
    protocol.emit("progress", progress=100, stage="download_model", model=model_name)
    return path, expected_sha256


def transcribe(request, protocol):
    started = time.monotonic()
    import numpy as np
    import torch
    import whisper
    if request["device"] == "cuda" and not torch.cuda.is_available():
        raise RuntimeError("CUDA 不可用，请运行资源检测或切换 CPU 配置")
    torch.set_num_threads(request.get("threads", 4))
    if request["device"] == "cuda":
        torch.cuda.reset_peak_memory_stats()
    protocol.emit("progress", progress=0, stage="verify_model", chunk_done=0, chunk_total=0)
    model_path, model_hash = ensure_model(request, protocol)
    options = dict(request, engine_version=whisper.__version__, model_sha256=model_hash)
    key = checkpoint_key(request["audio_path"], options)
    checkpoint_dir = Path(request["checkpoint_dir"]) / key
    checkpoint_dir.mkdir(parents=True, exist_ok=True)
    all_segments, resumed = [], 0
    model = None
    with wave.open(str(request["audio_path"]), "rb") as source:
        if (source.getnchannels() != 1 or source.getframerate() != 16000
                or source.getsampwidth() != 2 or source.getcomptype() != "NONE"):
            raise ValueError("worker 需要单声道 16kHz PCM16 WAV，请先用 FFmpeg 转换")
        rate = source.getframerate()
        chunks = plan_chunks(source.getnframes() / rate, request.get("chunk_seconds", 300))
        for index, (start, end) in enumerate(chunks):
            start_ms, end_ms = round(start * 1000), round(end * 1000)
            checkpoint = checkpoint_dir / f"{index:06d}.json"
            segments = read_checkpoint(checkpoint, key, index, start_ms, end_ms)
            if segments is None:
                if model is None:
                    protocol.emit("progress", progress=index / len(chunks) * 100,
                                  stage="load_model", chunk_done=index, chunk_total=len(chunks))
                    model = whisper.load_model(str(model_path), device=request["device"])
                protocol.emit("progress", progress=index / len(chunks) * 100,
                              stage="transcribe", chunk_done=index, chunk_total=len(chunks))
                source.setpos(round(start * rate))
                pcm = source.readframes(round(end * rate) - round(start * rate))
                audio = np.frombuffer(pcm, np.int16).astype(np.float32) / 32768.0
                result = model.transcribe(audio, language=request.get("language") or None,
                                          initial_prompt=request.get("prompt") or None,
                                          fp16=request["device"] == "cuda", verbose=False,
                                          condition_on_previous_text=True)
                segments = normalize_segments(result["segments"], start_ms, end_ms, f"{key}:{index}")
                atomic_json(checkpoint, {"protocol_version": 1, "key": key, "index": index,
                            "start_ms": start_ms, "end_ms": end_ms, "segments": segments})
            else:
                resumed += 1
            all_segments.extend(segments)
            for segment in segments:
                protocol.emit("segment", segment=segment)
            protocol.emit("checkpoint", chunk_done=index + 1, chunk_total=len(chunks), key=key)
            protocol.emit("progress", progress=(index + 1) / len(chunks) * 100,
                          stage="transcribe", chunk_done=index + 1, chunk_total=len(chunks))
    return {"segments": all_segments, "model": request["model"], "device": request["device"],
            "language": request.get("language") or "auto", "engine_version": whisper.__version__,
            "model_sha256": model_hash, "checkpoint_key": key, "resumed_chunks": resumed,
            "elapsed_seconds": round(time.monotonic() - started, 3), "peak_ram_mb": peak_ram_mb(),
            "peak_gpu_mb": round(torch.cuda.max_memory_allocated() / 1024 ** 2, 1)
                if request["device"] == "cuda" else 0,
            "peak_gpu_reserved_mb": round(torch.cuda.max_memory_reserved() / 1024 ** 2, 1)
                if request["device"] == "cuda" else 0,
            "gpu_metric": "pytorch_allocator_peak_not_system_gpu_usage"}


def main():
    if hasattr(sys.stdout, "reconfigure"):
        sys.stdout.reconfigure(encoding="utf-8", errors="strict")
        sys.stderr.reconfigure(encoding="utf-8", errors="replace")
        sys.stdin.reconfigure(encoding="utf-8")
    protocol = Protocol(sys.stdout)
    try:
        line = sys.stdin.readline(1024 * 1024 + 1)
        if len(line) > 1024 * 1024:
            raise ValueError("worker 请求过长")
        request = json.loads(line)
        validate_request(request)
        protocol.job_id = str(request.get("job_id", ""))
        protocol.emit("hello", worker="openai-whisper", version="0.1.0")
        with contextlib.redirect_stdout(sys.stderr):
            if request["command"] == "probe":
                result = probe(request)
            elif request["command"] == "download_model":
                path, checksum = ensure_model(request, protocol)
                result = {"model": request["model"], "path": str(path), "sha256": checksum,
                          "size_bytes": path.stat().st_size}
            else:
                result = transcribe(request, protocol)
        protocol.emit("done", **result)
        return 0
    except Exception as error:
        message = str(error)
        code = "worker_error"
        if "out of memory" in message.lower() or "cuda error: memory" in message.lower():
            code = "out_of_memory"
        elif isinstance(error, (FileNotFoundError, ImportError)):
            code = "missing_dependency"
        elif isinstance(error, (ValueError, json.JSONDecodeError)):
            code = "invalid_request"
        protocol.emit("error", code=code, message=message,
                      retryable=code not in ("invalid_request", "missing_dependency"))
        print(f"{type(error).__name__}: {message}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
