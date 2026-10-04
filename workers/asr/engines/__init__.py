"""Lazy engine selection; importing this module loads no ML dependency."""
from .common import request_options


def get_engine(request):
    if request.get("engine", "openai-whisper") == "openai-whisper":
        from .openai import OpenAIEngine
        return OpenAIEngine(request)
    if request.get("engine") == "faster-whisper":
        from .faster import FasterEngine
        return FasterEngine(request)
    raise ValueError("不支持的语音引擎")
