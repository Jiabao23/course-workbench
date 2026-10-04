"""Small engine-independent helpers, intentionally standard-library only."""
import hashlib
import importlib.metadata
import platform
import sys

DEFAULT_TEMPERATURE = [0.0, 0.2, 0.4, 0.6, 0.8, 1.0]


def request_options(request):
    engine = request.get("engine", "openai-whisper")
    return {"engine": engine,
            "compute_type": request.get("compute_type", "auto"),
            "beam_size": request.get("beam_size", 1),
            "best_of": request.get("best_of", None if engine == "openai-whisper" else 5),
            "temperature": request.get("temperature", list(DEFAULT_TEMPERATURE)),
            "condition_on_previous_text": request.get("condition_on_previous_text", True),
            "decoding_policy": request.get("decoding_policy", "standard-v1"),
            "chunk_strategy": request.get("chunk_strategy", "fixed"),
            "context_ms": request.get("context_ms", 2000),
            "chunk_seconds": request.get("chunk_seconds", 300),
            "threads": request.get("threads", 4)}


def decoding_options(request):
    options = request_options(request)
    result = {key: options[key] for key in ("beam_size", "best_of", "temperature",
              "condition_on_previous_text", "decoding_policy")}
    result.update(word_timestamps=options["chunk_strategy"] == "speech-boundary",
                  vad_filter=False, batch_size=1, concurrency=1,
                  task="transcribe", compression_ratio_threshold=2.4,
                  log_prob_threshold=-1.0, no_speech_threshold=0.6,
                  language=request.get("language") or None,
                  initial_prompt=request.get("prompt") or None)
    return result


def runtime_versions(packages):
    versions = {"python": platform.python_version(), "executable": sys.executable}
    for distribution in packages:
        try:
            versions[distribution] = importlib.metadata.version(distribution)
        except importlib.metadata.PackageNotFoundError:
            versions[distribution] = None
    return versions


def file_sha256(path):
    digest = hashlib.sha256()
    with open(path, "rb") as stream:
        for block in iter(lambda: stream.read(4 * 1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()
