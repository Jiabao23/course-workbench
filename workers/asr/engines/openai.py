"""Original PyTorch Whisper implementation with explicit measured decoding."""
import importlib
from pathlib import Path

from .common import decoding_options, file_sha256, runtime_versions


class OpenAIEngine:
    def __init__(self, request):
        self.request = request
        self.torch = importlib.import_module("torch")
        self.whisper = importlib.import_module("whisper")
        self.engine_version = str(self.whisper.__version__)
        self.runtime_versions = runtime_versions(("openai-whisper", "torch", "numpy", "tiktoken"))
        self.runtime_versions["cuda"] = self.torch.version.cuda
        self.device = request.get("device", "cpu")
        self.compute_type = request.get("compute_type", "auto")
        if self.compute_type == "auto":
            self.compute_type = "float16" if self.device == "cuda" else "float32"
        if self.compute_type not in ("float32", "float16") or (
                self.device == "cpu" and self.compute_type != "float32"):
            raise ValueError("OpenAI Whisper 不支持此设备的计算类型")
        if self.device == "cuda" and not self.torch.cuda.is_available():
            raise RuntimeError("CUDA 不可用，请运行资源检测或切换 CPU 配置")
        self.torch.set_num_threads(request.get("threads", 4))
        if self.device == "cuda":
            self.torch.cuda.reset_peak_memory_stats()
        self.decode_options = decoding_options(request)
        # Whisper's legacy default is greedy decoding (beam_size=None), not a
        # one-beam BeamSearchDecoder. Keep effective options faithful to the call.
        if self.decode_options["beam_size"] == 1:
            self.decode_options["beam_size"] = None
        self.warnings = []

    def ensure_model(self, request, protocol):
        model_name = request["model"]
        if model_name not in self.whisper.available_models():
            raise ValueError(f"当前 Whisper 版本不支持模型 {model_name}")
        directory = Path(request["model_dir"]).expanduser().resolve()
        directory.mkdir(parents=True, exist_ok=True)
        url = self.whisper._MODELS[model_name]
        path = directory / url.rsplit("/", 1)[-1]
        expected = url.split("/")[-2]
        if path.is_file() and file_sha256(path) == expected:
            return path, expected
        if not request.get("allow_download", False) and request["command"] != "download_model":
            error = "模型校验失败" if path.exists() else "模型尚未下载"
            raise FileNotFoundError(f"{error}：{model_name}。请在设置中下载或选择已有模型目录。")
        protocol.emit("progress", progress=0, stage="download_model", model=model_name)
        self.whisper._download(url, str(directory), in_memory=False)
        if not path.is_file() or file_sha256(path) != expected:
            raise RuntimeError("下载的模型未通过 SHA256 校验，请重试")
        protocol.emit("progress", progress=100, stage="download_model", model=model_name)
        return path, expected

    def load_model(self, path):
        return self.whisper.load_model(str(path), device=self.device)

    def transcribe(self, model, audio):
        options = self.decode_options
        return model.transcribe(audio, language=options["language"],
            initial_prompt=options["initial_prompt"], fp16=self.compute_type == "float16",
            verbose=False, condition_on_previous_text=options["condition_on_previous_text"],
            beam_size=options["beam_size"], best_of=options["best_of"],
            temperature=options["temperature"], word_timestamps=options["word_timestamps"],
            compression_ratio_threshold=options["compression_ratio_threshold"],
            logprob_threshold=options["log_prob_threshold"],
            no_speech_threshold=options["no_speech_threshold"])

    def synchronize(self):
        if self.device == "cuda":
            self.torch.cuda.synchronize()

    def metrics(self):
        return {"peak_gpu_mb": round(self.torch.cuda.max_memory_allocated() / 1024 ** 2, 1)
                    if self.device == "cuda" else 0,
                "peak_gpu_reserved_mb": round(self.torch.cuda.max_memory_reserved() / 1024 ** 2, 1)
                    if self.device == "cuda" else 0,
                "gpu_metric": "pytorch_allocator_peak_not_system_gpu_usage"}
