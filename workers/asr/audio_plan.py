"""Pure, versioned core/decode interval planning. No silence is discarded."""
import hashlib
import json
import math

PLANNER_VERSION = "speech-boundary-v2-silent-context"
FIXED_PLANNER_VERSION = "fixed-v1"


def seconds_to_ms(seconds):
    """Match Rust's ties-away rounding instead of Python's ties-to-even."""
    scaled = seconds * 1000
    return math.floor(scaled + 0.5) if scaled >= 0 else math.ceil(scaled - 0.5)


def build_manifest(duration_ms, chunk_seconds=300, strategy="fixed", speech_intervals_ms=None,
                   context_ms=2000):
    if type(duration_ms) is not int or duration_ms <= 0:
        raise ValueError("音频时长必须大于 0 毫秒")
    if (isinstance(chunk_seconds, bool) or not isinstance(chunk_seconds, (int, float))
            or not math.isfinite(chunk_seconds) or chunk_seconds <= 0):
        raise ValueError("分块时长必须大于 0")
    target_ms = seconds_to_ms(chunk_seconds)
    if target_ms < 1 or strategy not in ("fixed", "speech-boundary"):
        raise ValueError("不支持的分块策略或时长")
    if type(context_ms) is not int or not 0 <= context_ms <= 10000:
        raise ValueError("上下文必须为 0 到 10000 毫秒")
    if strategy == "speech-boundary" and target_ms > 360000:
        raise ValueError("实验分块核心区间不得超过 360 秒")
    intervals = []
    if speech_intervals_ms is not None:
        if not isinstance(speech_intervals_ms, list):
            raise ValueError("语音区间必须为毫秒起止数组")
        for item in speech_intervals_ms:
            if (not isinstance(item, (list, tuple)) or len(item) != 2
                    or any(type(value) is not int for value in item)
                    or not 0 <= item[0] < item[1] <= duration_ms):
                raise ValueError("语音区间必须为有效的毫秒起止范围")
        for start, end in sorted(speech_intervals_ms):
            if intervals and start <= intervals[-1][1]:
                intervals[-1][1] = max(intervals[-1][1], end)
            else:
                intervals.append([start, end])
    # An absent detector result is unknown, never evidence of silence.
    pauses = []
    if speech_intervals_ms is not None:
        previous = 0
        for start, end in intervals:
            if start > previous:
                pauses.append((previous, start))
            previous = end
        if previous < duration_ms:
            pauses.append((previous, duration_ms))
    result, start = [], 0
    while start < duration_ms:
        end = min(start + target_ms, duration_ms)
        if strategy == "speech-boundary" and end < duration_ms:
            lower = max(start + 1, end - min(60000, target_ms // 5))
            upper = min(duration_ms - 1, start + 360000, end + 60000)
            candidates = []
            for pause_start, pause_end in pauses:
                # A gap's center avoids cutting close to either spoken edge.
                midpoint = (pause_start + pause_end) // 2
                if lower <= midpoint <= upper:
                    candidates.append(midpoint)
            if candidates:
                end = min(candidates, key=lambda value: (abs(value - end), value))
        overlap = context_ms if strategy == "speech-boundary" else 0
        result.append({"index": len(result), "start_ms": start, "end_ms": end,
                       "decode_start_ms": max(0, start - overlap),
                       "decode_end_ms": min(duration_ms, end + overlap)})
        start = end
    return result


def manifest_hash(manifest, planner_version):
    data = json.dumps({"planner_version": planner_version, "manifest": manifest},
                      sort_keys=True, separators=(",", ":")).encode("utf-8")
    return hashlib.sha256(data).hexdigest()
