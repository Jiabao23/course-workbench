import io
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from test_worker import worker


class CheckpointAndProtocolTests(unittest.TestCase):
    def test_checkpoint_preserves_diagnostics_and_rejects_non_numeric_evidence(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "chunk.json"
            segment = {"id": "s", "start_ms": 0, "end_ms": 1000, "text": "课程",
                       "diagnostics": {"avg_logprob": -0.7}}
            data = {"protocol_version": 1, "key": "k", "index": 0,
                    "start_ms": 0, "end_ms": 1000, "segments": [segment]}
            worker.atomic_json(path, data)
            self.assertEqual(worker.read_checkpoint(path, "k", 0, 0, 1000), [segment])
            for invalid in ({"avg_logprob": True}, {"avg_logprob": "bad"}, []):
                segment["diagnostics"] = invalid
                worker.atomic_json(path, data)
                self.assertIsNone(worker.read_checkpoint(path, "k", 0, 0, 1000))

    def test_atomic_checkpoint_accepts_only_matching_configuration_and_bounds(self):
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "checkpoints" / "000001.json"
            segments = [{"id": "s1", "start_ms": 300000, "end_ms": 301000, "text": "网络"}]
            checkpoint = {"protocol_version": 1, "key": "config-a", "index": 1,
                          "start_ms": 300000, "end_ms": 302000, "segments": segments}
            worker.atomic_json(path, checkpoint)
            self.assertEqual(segments, worker.read_checkpoint(path, "config-a", 1, 300000, 302000))
            self.assertIsNone(worker.read_checkpoint(path, "config-b", 1, 300000, 302000))
            self.assertIsNone(worker.read_checkpoint(path, "config-a", 2, 300000, 302000))
            checkpoint["segments"][0]["end_ms"] = 400000
            worker.atomic_json(path, checkpoint)
            self.assertIsNone(worker.read_checkpoint(path, "config-a", 1, 300000, 302000))
            self.assertEqual(list(path.parent.glob("*.tmp")), [])
            path.write_text('{"partial":', encoding="utf-8")
            self.assertIsNone(worker.read_checkpoint(path, "config-a", 1, 300000, 302000))

    def test_jsonl_preserves_unicode_without_protocol_noise(self):
        stream = io.StringIO()
        protocol = worker.Protocol(stream, "job-a")
        protocol.emit("segment", segment={"text": "课程\n目标"})
        lines = stream.getvalue().splitlines()
        self.assertEqual(len(lines), 1)
        self.assertEqual(json.loads(lines[0])["segment"]["text"], "课程\n目标")

    def test_bad_request_returns_structured_error_and_nonzero_exit(self):
        result = subprocess.run([sys.executable, str(Path(__file__).parents[1] / "worker.py")],
                                input='{"protocol_version":999,"command":"probe"}\n',
                                text=True, encoding="utf-8", capture_output=True, timeout=15)
        self.assertNotEqual(result.returncode, 0)
        messages = [json.loads(line) for line in result.stdout.splitlines()]
        self.assertEqual(len(messages), 1)
        self.assertEqual(messages[0]["type"], "error")
        self.assertEqual(messages[0]["code"], "invalid_request")

    def test_empty_or_out_of_bounds_segments_are_not_committed(self):
        result = worker.normalize_segments([
            {"start": 10, "end": 11, "text": "越界"},
            {"start": 1, "end": 0, "text": "倒置"},
            {"start": float("nan"), "end": 2, "text": "错误"}], 0, 3000, "k")
        self.assertEqual(result, [])


if __name__ == "__main__":
    unittest.main()
