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

# Also support direct file imports used by the standard-library test suite.
_WORKER_DIR = str(Path(__file__).resolve().parent)
if _WORKER_DIR not in sys.path:
    sys.path.insert(0, _WORKER_DIR)
from audio_plan import (build_manifest, manifest_hash, PLANNER_VERSION,
                        FIXED_PLANNER_VERSION, seconds_to_ms)
from engines import get_engine, request_options

PROTOCOL_VERSION = 1
MODELS = ("tiny", "tiny.en", "base", "base.en", "small", "small.en", "medium",
          "medium.en", "large", "large-v1", "large-v2", "large-v3", "turbo",
          "large-v3-turbo")
IDENTITY_FIELDS = ("model", "device", "language", "prompt", "chunk_seconds",
                   "threads", "engine_version", "model_sha256", "engine", "compute_type",
                   "requested_compute_type", "runtime_versions", "beam_size", "best_of",
                   "temperature", "condition_on_previous_text", "decoding_policy",
                   "decode_options", "chunk_strategy", "context_ms", "planner_version",
                   "manifest_sha256")


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def checkpoint_key(audio_path, options, audio_sha256=None):
    effective = dict(request_options(options), **options)
    identity = {key: effective.get(key) for key in IDENTITY_FIELDS}
    identity.update(protocol_version=PROTOCOL_VERSION,
                    audio_sha256=audio_sha256 or file_sha256(audio_path), normalizer_version=3)
    encoded = json.dumps(identity, sort_keys=True, ensure_ascii=False, allow_nan=False).encode("utf-8")
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
        start_ms = max(offset_ms, min(duration_ms, offset_ms + seconds_to_ms(start)))
        end_ms = max(start_ms, min(duration_ms, offset_ms + seconds_to_ms(end)))
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


def normalize_owned_segments(segments, decode_start_ms, core_start_ms, core_end_ms, prefix,
                             decode_end_ms=None):
    """Assign every timestamped word by its midpoint, preserving real repeats.

    Unknown or incomplete alignment is a hard experimental failure. Guessing at
    whole-segment ownership would silently lose words at a contextual boundary.
    """
    owned = []
    for item in segments:
        text = str(item.get("text", ""))
        if not text.strip():
            continue
        words = item.get("words")
        error = "实验语音边界分块需要完整且有效的词级时间戳；请改用固定分块并复核边界"
        if not isinstance(words, list) or not words:
            raise ValueError(error)
        aligned_text = "".join(str(word.get("word", "")) for word in words
                               if isinstance(word, dict))
        if "".join(aligned_text.split()) != "".join(text.split()):
            raise ValueError(error)
        selected, previous = [], -math.inf
        for word in words:
            if not isinstance(word, dict) or not isinstance(word.get("word"), str):
                raise ValueError(error)
            start, end = word.get("start"), word.get("end")
            if (any(isinstance(value, bool) or not isinstance(value, (float, int))
                    or not math.isfinite(value) for value in (start, end))
                    or not max(0, previous) <= start < end
                    or (decode_end_ms is not None and decode_start_ms + end * 1000 > decode_end_ms + 1)):
                raise ValueError(error)
            previous = start
            start_ms = decode_start_ms + seconds_to_ms(start)
            end_ms = decode_start_ms + seconds_to_ms(end)
            midpoint = decode_start_ms + (start + end) * 500
            if core_start_ms <= midpoint < core_end_ms:
                if min(core_end_ms, end_ms) <= max(core_start_ms, start_ms):
                    raise ValueError(error)
                selected.append((max(core_start_ms, start_ms),
                                 min(core_end_ms, end_ms), word["word"]))
        if selected:
            owned.append(dict(item, start=selected[0][0] / 1000,
                              end=max(w[1] for w in selected) / 1000,
                              text="".join(w[2] for w in selected)))
    return normalize_segments(owned, 0, core_end_ms, prefix)


def require_silent_boundary_context(segments, decode_start_ms, chunk, duration_ms, context_ms):
    """Fail closed when independently decoded contexts could disagree on ownership.

    Called after complete word-timestamp validation and before checkpoint commit.
    Both neighboring decodes must contain no word touching their shared context;
    midpoint ownership alone cannot resolve independently shifted timestamps.
    """
    boundaries = [edge for edge in (chunk["start_ms"], chunk["end_ms"])
                  if 0 < edge < duration_ms]
    for segment in segments:
        for word in segment.get("words", []):
            start = decode_start_ms + word["start"] * 1000
            end = decode_start_ms + word["end"] * 1000
            if any(start <= edge + context_ms and end >= edge - context_ms for edge in boundaries):
                raise ValueError("实验分块边界存在讲话/对齐歧义，请固定分块；未提交歧义边界结果")


def validate_request(request):
    if not isinstance(request, dict) or request.get("protocol_version") != PROTOCOL_VERSION:
        raise ValueError("不支持的 worker 协议版本")
    if request.get("command") not in ("probe", "transcribe", "download_model"):
        raise ValueError("未知 worker 命令")
    options = request_options(request)
    if options["engine"] not in ("openai-whisper", "faster-whisper"):
        raise ValueError("不支持的语音引擎")
    if options["compute_type"] not in ("auto", "float32", "float16", "int8", "int8_float16"):
        raise ValueError("不支持的计算类型")
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
    if type(request.get("threads", 4)) is not int or not 1 <= request.get("threads", 4) <= 256:
        raise ValueError("线程数必须为 1 到 256")
    if options["engine"] == "openai-whisper" and (options["compute_type"] in ("int8", "int8_float16")
            or (request["device"] == "cpu" and options["compute_type"] == "float16")):
        raise ValueError("OpenAI Whisper 不支持此设备的计算类型")
    for key in ("beam_size", "best_of"):
        if key == "best_of" and key not in request and options["engine"] == "openai-whisper":
            continue
        if type(options[key]) is not int or not 1 <= options[key] <= 5:
            raise ValueError(f"{key} 必须为 1 到 5 的整数")
    temperature = options["temperature"]
    values = temperature if isinstance(temperature, list) else [temperature]
    if not 1 <= len(values) <= 10 or any(isinstance(value, bool)
            or not isinstance(value, (int, float)) or not math.isfinite(value)
            or not 0 <= value <= 1 for value in values):
        raise ValueError("temperature 必须为 0 到 1 的数值或非空数值数组")
    if not isinstance(options["condition_on_previous_text"], bool):
        raise ValueError("condition_on_previous_text 必须为布尔值")
    if not isinstance(options["decoding_policy"], str) or not 1 <= len(options["decoding_policy"]) <= 100:
        raise ValueError("decoding_policy 必须为 1 到 100 字符的策略名称")
    if options["chunk_strategy"] not in ("fixed", "speech-boundary"):
        raise ValueError("不支持的分块策略")
    if type(options["context_ms"]) is not int or not 0 <= options["context_ms"] <= 10000:
        raise ValueError("context_ms 必须为 0 到 10000 毫秒")
    chunk = request.get("chunk_seconds", 300)
    if isinstance(chunk, bool) or not isinstance(chunk, (int, float)) or not math.isfinite(chunk) or not 30 <= chunk <= 1800:
        raise ValueError("分块时长必须为 30 到 1800 秒")
    if options["chunk_strategy"] == "speech-boundary" and chunk > 360:
        raise ValueError("实验分块核心区间不得超过 360 秒")
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


def read_checkpoint(path, key, index, start_ms, end_ms, identity=None):
    try:
        item = json.loads(Path(path).read_text(encoding="utf-8"))
        if (any(type(item.get(name)) is not int for name in
                ("protocol_version", "index", "start_ms", "end_ms"))
                or item.get("protocol_version") != 1 or item.get("key") != key
                or item.get("index") != index or item.get("start_ms") != start_ms
                or item.get("end_ms") != end_ms or not isinstance(item.get("segments"), list)
                or (identity is not None and item.get("identity") != identity)):
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
                    or not isinstance(segment.get("id"), str) or not segment["id"]
                    or segment["id"] in identifiers
                    or type(segment.get("start_ms")) is not int
                    or type(segment.get("end_ms")) is not int
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
    if request.get("engine") == "faster-whisper":
        from engines.faster import probe as faster_probe
        return faster_probe(request)
    dependencies = {name: importlib.util.find_spec(name) is not None
                    for name in ("torch", "whisper", "numpy", "yt_dlp")}
    info = {"python_version": platform.python_version(), "executable": sys.executable,
            "dependencies": dependencies, "torch_version": None, "cuda_version": None,
            "cuda_available": False, "gpu_name": None, "gpu_total_mb": 0, "gpu_free_mb": 0,
            "models": [], "warnings": [], "engine": "openai-whisper",
            "supported_compute_types": {"cpu": ["float32"], "cuda": []}}
    from engines.common import runtime_versions
    info["runtime_versions"] = runtime_versions(("openai-whisper", "torch", "numpy", "tiktoken"))
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
                info["supported_compute_types"]["cuda"] = ["float16", "float32"]
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
    if request.get("engine") == "faster-whisper":
        from engines.faster import download_model
        return download_model(request, protocol)
    return get_engine(request).ensure_model(request, protocol)


def transcribe(request, protocol):
    started = time.monotonic()
    timings = {key: 0.0 for key in ("dependency_seconds", "model_verify_seconds",
               "audio_hash_seconds", "model_load_seconds", "inference_seconds", "checkpoint_seconds")}
    mark = time.monotonic()
    import numpy as np
    engine = get_engine(request)
    timings["dependency_seconds"] = time.monotonic() - mark
    protocol.emit("progress", progress=0, stage="verify_model", chunk_done=0, chunk_total=0)
    mark = time.monotonic()
    model_path, model_hash = engine.ensure_model(request, protocol)
    timings["model_verify_seconds"] = time.monotonic() - mark
    mark = time.monotonic()
    audio_hash = file_sha256(request["audio_path"])
    timings["audio_hash_seconds"] = time.monotonic() - mark
    options = dict(request_options(request), **request)
    options.update(engine_version=engine.engine_version, model_sha256=model_hash,
                   runtime_versions=engine.runtime_versions,
                   requested_compute_type=request.get("compute_type", "auto"),
                   compute_type=engine.compute_type, decode_options=engine.decode_options)
    all_segments, completed, resumed = [], [], 0
    model = None
    with wave.open(str(request["audio_path"]), "rb") as source:
        if (source.getnchannels() != 1 or source.getframerate() != 16000
                or source.getsampwidth() != 2 or source.getcomptype() != "NONE"
                or source.getnframes() == 0):
            raise ValueError("worker 需要非空单声道 16kHz PCM16 WAV，请先用 FFmpeg 转换")
        rate, frames = source.getframerate(), source.getnframes()
        duration_ms = max(1, (frames * 1000 + rate // 2) // rate)
        manifest = build_manifest(duration_ms, options["chunk_seconds"], options["chunk_strategy"],
                                  request.get("speech_intervals_ms"), options["context_ms"])
        planner_version = PLANNER_VERSION if options["chunk_strategy"] == "speech-boundary" else FIXED_PLANNER_VERSION
        plan_hash = manifest_hash(manifest, planner_version)
        options.update(planner_version=planner_version, manifest_sha256=plan_hash)
        key = checkpoint_key(request["audio_path"], options, audio_sha256=audio_hash)
        checkpoint_dir = Path(request["checkpoint_dir"]) / key
        checkpoint_dir.mkdir(parents=True, exist_ok=True)
        identity = {name: options.get(name) for name in IDENTITY_FIELDS}
        identity.update(audio_sha256=audio_hash, normalizer_version=3)
        mark = time.monotonic()
        atomic_json(checkpoint_dir / "manifest.json", {"protocol_version": 1, "key": key,
                    "identity": identity, "manifest": manifest})
        timings["checkpoint_seconds"] += time.monotonic() - mark
        for chunk in manifest:
            index, start_ms, end_ms = chunk["index"], chunk["start_ms"], chunk["end_ms"]
            checkpoint = checkpoint_dir / f"{index:06d}.json"
            mark = time.monotonic()
            segments = read_checkpoint(checkpoint, key, index, start_ms, end_ms, identity)
            timings["checkpoint_seconds"] += time.monotonic() - mark
            if segments is None:
                if model is None:
                    protocol.emit("progress", progress=index / len(manifest) * 100,
                                  stage="load_model", chunk_done=index, chunk_total=len(manifest))
                    engine.synchronize()
                    mark = time.monotonic()
                    model = engine.load_model(model_path)
                    engine.synchronize()
                    timings["model_load_seconds"] += time.monotonic() - mark
                protocol.emit("progress", progress=index / len(manifest) * 100,
                              stage="transcribe", chunk_done=index, chunk_total=len(manifest))
                first_frame = min(frames, (chunk["decode_start_ms"] * rate + 500) // 1000)
                last_frame = frames if chunk["decode_end_ms"] == duration_ms else min(
                    frames, (chunk["decode_end_ms"] * rate + 500) // 1000)
                source.setpos(first_frame)
                pcm = source.readframes(last_frame - first_frame)
                if len(pcm) != (last_frame - first_frame) * 2:
                    raise ValueError("WAV 音频数据已截断，不能将缺失输入标记为完成")
                audio = np.frombuffer(pcm, np.int16).astype(np.float32) / 32768.0
                engine.synchronize()
                mark = time.monotonic()
                result = engine.transcribe(model, audio)
                engine.synchronize()
                timings["inference_seconds"] += time.monotonic() - mark
                del audio, pcm
                if options["chunk_strategy"] == "speech-boundary":
                    segments = normalize_owned_segments(result["segments"], chunk["decode_start_ms"],
                                                        start_ms, end_ms, f"{key}:{index}",
                                                        decode_end_ms=chunk["decode_end_ms"])
                    require_silent_boundary_context(result["segments"], chunk["decode_start_ms"],
                                                    chunk, duration_ms, options["context_ms"])
                else:
                    segments = normalize_segments(result["segments"], start_ms, end_ms, f"{key}:{index}")
                mark = time.monotonic()
                atomic_json(checkpoint, {"protocol_version": 1, "key": key, **chunk,
                                        "identity": identity, "segments": segments})
                timings["checkpoint_seconds"] += time.monotonic() - mark
            else:
                resumed += 1
            completed.append(index)
            all_segments.extend(segments)
            for segment in segments:
                protocol.emit("segment", segment=segment)
            protocol.emit("checkpoint", chunk_done=index + 1, chunk_total=len(manifest), key=key,
                          completed_chunk_index=index, manifest_sha256=plan_hash)
            protocol.emit("progress", progress=(index + 1) / len(manifest) * 100,
                          stage="transcribe", chunk_done=index + 1, chunk_total=len(manifest))
    timings["total_seconds"] = time.monotonic() - started
    return {"segments": all_segments, "model": request["model"], "device": request["device"],
            "language": request.get("language") or "auto", "engine": options["engine"],
            "engine_version": engine.engine_version, "runtime_versions": engine.runtime_versions,
            "requested_compute_type": options["requested_compute_type"], "compute_type": engine.compute_type,
            "decode_options": engine.decode_options, "audio_sha256": audio_hash,
            "model_sha256": model_hash, "checkpoint_key": key, "resumed_chunks": resumed,
            "model_loaded": model is not None, "chunk_strategy": options["chunk_strategy"],
            "planner_version": planner_version, "manifest_sha256": plan_hash, "manifest": manifest,
            "completed_chunk_indices": completed, "duration_ms": duration_ms,
            "timings": {name: round(value, 6) for name, value in timings.items()},
            "elapsed_seconds": round(timings["total_seconds"], 3), "peak_ram_mb": peak_ram_mb(),
            **engine.metrics(), "warnings": list(engine.warnings)}


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
        protocol.emit("hello", worker=request.get("engine", "openai-whisper"), version="0.2.0")
        with contextlib.redirect_stdout(sys.stderr):
            if request["command"] == "probe":
                result = probe(request)
            elif request["command"] == "download_model":
                path, checksum = ensure_model(request, protocol)
                result = {"model": request["model"], "path": str(path), "sha256": checksum,
                          "size_bytes": sum(item.stat().st_size for item in path.rglob("*") if item.is_file())
                              if path.is_dir() else path.stat().st_size,
                          "engine": request.get("engine", "openai-whisper")}
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
