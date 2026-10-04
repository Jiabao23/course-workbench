"""Optional CTranslate2 adapter. All inference models are strictly local."""
import hashlib
import importlib
import json
import os
from pathlib import Path
import sys
import tempfile
import uuid

from .common import decoding_options, file_sha256, runtime_versions

OFFICIAL_MODELS = ("tiny", "tiny.en", "base", "base.en", "small", "small.en",
                   "medium", "medium.en", "large-v1", "large-v2", "large-v3")
_DLL_HANDLES = []
_DLL_DIRECTORY = None


def register_local_cuda():
    """Only this child process sees the runtime's optional CUDA DLL directory."""
    global _DLL_DIRECTORY
    directory = Path(sys.prefix) / "cuda"
    if sys.platform != "win32" or not directory.is_dir() or _DLL_DIRECTORY == str(directory):
        return
    if hasattr(os, "add_dll_directory"):
        _DLL_HANDLES.append(os.add_dll_directory(str(directory)))
    os.environ["PATH"] = str(directory) + os.pathsep + os.environ.get("PATH", "")
    _DLL_DIRECTORY = str(directory)


def canonical_model(name):
    if name == "large":
        return "large-v3"
    if name not in OFFICIAL_MODELS:
        raise ValueError(f"当前 faster-whisper 适配器没有官方 Systran 模型映射：{name}")
    return name


def model_files(directory):
    directory = Path(directory)
    required = ["model.bin", "config.json", "tokenizer.json"]
    vocabularies = [name for name in ("vocabulary.json", "vocabulary.txt")
                    if (directory / name).is_file()]
    if (not vocabularies or any(not (directory / name).is_file()
                              or (directory / name).stat().st_size == 0 for name in required)
            or any(directory.rglob("*.incomplete"))):
        raise FileNotFoundError("faster-whisper 模型不完整，请先在设置中下载官方模型")
    names = required + vocabularies
    if (directory / "preprocessor_config.json").is_file():
        names.append("preprocessor_config.json")
    for name in sorted(names):
        path = directory / name
        if path.stat().st_size == 0:
            raise FileNotFoundError(f"faster-whisper 模型文件为空：{name}")
        if name.endswith(".json"):
            try:
                json.loads(path.read_text(encoding="utf-8"))
            except (OSError, ValueError) as error:
                raise FileNotFoundError(f"faster-whisper 模型配置损坏：{name}") from error
    return sorted(names)


def model_identity(directory):
    directory = Path(directory)
    digest = hashlib.sha256()
    for name in model_files(directory):
        digest.update(name.encode("utf-8") + b"\0" + file_sha256(directory / name).encode("ascii") + b"\n")
    return digest.hexdigest()


def download_model(request, protocol):
    """Download to a sibling staging directory; publish only validated files."""
    name = canonical_model(request["model"])
    directory = Path(request["model_dir"]).expanduser().resolve()
    path = directory / name
    try:
        return path, model_identity(path)
    except FileNotFoundError:
        if request["command"] != "download_model":
            raise
    hub = importlib.import_module("huggingface_hub")
    directory.mkdir(parents=True, exist_ok=True)
    protocol.emit("progress", progress=0, stage="download_model", model=name)
    with tempfile.TemporaryDirectory(dir=directory, prefix=".cw-download-") as temporary:
        staged = Path(temporary) / name
        staged.mkdir()
        hub.snapshot_download(f"Systran/faster-whisper-{name}", local_dir=str(staged),
            local_files_only=False,
            allow_patterns=["model.bin", "config.json", "tokenizer.json", "vocabulary.*",
                            "preprocessor_config.json"])
        checksum = model_identity(staged)
        backup = None
        if path.exists():
            backup = directory / f".{name}-invalid-{uuid.uuid4().hex}"
            os.replace(path, backup)
        try:
            os.replace(staged, path)
        except OSError:
            if backup is not None:
                os.replace(backup, path)
            raise
    protocol.emit("progress", progress=100, stage="download_model", model=name)
    return path, checksum


def probe(request):
    register_local_cuda()
    dependencies = {}
    for name in ("faster_whisper", "ctranslate2", "numpy", "yt_dlp"):
        try:
            dependencies[name] = importlib.util.find_spec(name) is not None
        except (ImportError, ValueError):
            dependencies[name] = False
    versions = runtime_versions(("faster-whisper", "ctranslate2", "numpy", "tokenizers", "huggingface-hub"))
    info = {"engine": "faster-whisper", "python_version": versions["python"],
            "executable": sys.executable, "dependencies": dependencies,
            "engine_version": versions["faster-whisper"], "runtime_versions": versions,
            "torch_version": None, "cuda_version": None, "cuda_available": False,
            "gpu_name": None, "gpu_total_mb": 0, "gpu_free_mb": 0, "models": [],
            "supported_compute_types": {"cpu": [], "cuda": []},
            "warnings": ["CUDA 能力探测不能证明推理可用；仍需实际加载模型验证 CUDA/cuDNN 兼容性。",
                         "CTranslate2 的 GPU 分配器峰值不可用；未使用 PyTorch 指标替代。"]}
    for name in ("faster_whisper", "ctranslate2", "numpy"):
        if not dependencies[name]:
            info["warnings"].append(f"独立 Python 环境缺少依赖：{name}")
    if all(dependencies[name] for name in ("faster_whisper", "ctranslate2", "numpy")):
        try:
            ct2 = importlib.import_module("ctranslate2")
            faster = importlib.import_module("faster_whisper")
            importlib.import_module("numpy")
            info["engine_version"] = str(faster.__version__)
            info["supported_compute_types"]["cpu"] = sorted(ct2.get_supported_compute_types("cpu"))
            info["cuda_available"] = ct2.get_cuda_device_count() > 0
            if info["cuda_available"]:
                info["supported_compute_types"]["cuda"] = sorted(ct2.get_supported_compute_types("cuda"))
        except Exception as error:
            info["warnings"].append(f"CTranslate2 能力探测失败：{error}")
            info["cuda_available"] = False
    directory = Path(request.get("model_dir", "."))
    for name in OFFICIAL_MODELS:
        path = directory / name
        if path.is_dir():
            try:
                names = model_files(path)
                info["models"].append({"name": name, "size_bytes": sum((path / n).stat().st_size for n in names)})
            except FileNotFoundError as error:
                info["warnings"].append(f"{name}：{error}")
    return info


class FasterEngine:
    def __init__(self, request):
        register_local_cuda()
        self.request = request
        self.ct2 = importlib.import_module("ctranslate2")
        self.faster = importlib.import_module("faster_whisper")
        self.engine_version = str(self.faster.__version__)
        self.runtime_versions = runtime_versions(("faster-whisper", "ctranslate2", "numpy",
                                                  "tokenizers", "huggingface-hub"))
        self.runtime_versions["cuda_dll_directory"] = _DLL_DIRECTORY
        self.device = request.get("device", "cpu")
        supported = self.ct2.get_supported_compute_types(self.device)
        self.compute_type = request.get("compute_type", "auto")
        if self.compute_type == "auto":
            self.compute_type = "float16" if self.device == "cuda" and "float16" in supported else "float32"
        if self.compute_type not in supported:
            raise ValueError(f"CTranslate2 在 {self.device} 上不支持 {self.compute_type}，支持：{', '.join(sorted(supported))}")
        self.decode_options = decoding_options(request)
        self.warnings = ["CTranslate2 的 GPU 分配器峰值不可用；未使用 PyTorch 指标替代。"]

    def ensure_model(self, request, protocol):
        name = canonical_model(request["model"])
        path = Path(request["model_dir"]).expanduser().resolve() / name
        try:
            return path, model_identity(path)
        except FileNotFoundError:
            if request["command"] != "download_model":
                raise FileNotFoundError(f"faster-whisper 模型尚未下载或不完整：{path}。请先在设置中下载；转写不会联网。") from None
        return download_model(request, protocol)

    def load_model(self, path):
        # Requiring tokenizer.json also prevents the upstream tokenizer fallback download.
        return self.faster.WhisperModel(str(path), device=self.device,
            compute_type=self.compute_type, cpu_threads=self.request.get("threads", 4),
            num_workers=1, local_files_only=True)

    def transcribe(self, model, audio):
        options = self.decode_options
        segments, info = model.transcribe(audio, language=options["language"],
            initial_prompt=options["initial_prompt"], beam_size=options["beam_size"],
            best_of=options["best_of"], temperature=options["temperature"],
            condition_on_previous_text=options["condition_on_previous_text"],
            word_timestamps=options["word_timestamps"], vad_filter=False,
            compression_ratio_threshold=options["compression_ratio_threshold"],
            log_prob_threshold=options["log_prob_threshold"],
            no_speech_threshold=options["no_speech_threshold"])
        # Inference happens while iterating. Returning the generator would lie about
        # timing/completion and could lose errors or partially written checkpoints.
        result = []
        for segment in segments:
            item = {key: getattr(segment, key) for key in ("start", "end", "text",
                    "avg_logprob", "no_speech_prob", "compression_ratio")}
            if segment.words is not None:
                item["words"] = [{key: getattr(word, key) for key in ("start", "end", "word")}
                                 for word in segment.words]
            result.append(item)
        return {"segments": result, "language": info.language}

    def synchronize(self):
        # CTranslate2 transcribe/generate results are synchronous after exhaustion.
        pass

    def metrics(self):
        return {"peak_gpu_mb": None, "peak_gpu_reserved_mb": None,
                "gpu_metric": "unavailable_ctranslate2_gpu_allocator_metrics"}
