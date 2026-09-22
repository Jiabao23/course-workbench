import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("cw_worker", Path(__file__).parents[1] / "worker.py")
worker = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(worker)


class WorkerBehaviorTests(unittest.TestCase):
    def test_optional_diagnostics_are_finite_numbers_and_old_segments_stay_unchanged(self):
        raw = {"start": 0, "end": 1, "text": "课程"}
        self.assertNotIn("diagnostics", worker.normalize_segments([raw], 0, 1000, "k")[0])
        parsed = worker.normalize_segments([dict(raw, avg_logprob=-0.8, no_speech_prob=0.2,
                                               compression_ratio=1.4)], 0, 1000, "k")[0]
        self.assertEqual(parsed["diagnostics"], {"avg_logprob": -0.8, "no_speech_prob": 0.2,
                                                "compression_ratio": 1.4})
        invalid = dict(raw, avg_logprob=float("nan"), no_speech_prob=True, compression_ratio="2")
        self.assertNotIn("diagnostics", worker.normalize_segments([invalid], 0, 1000, "k")[0])

    def test_checkpoint_identity_prevents_mixing_models_and_prompts(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "a.wav"
            audio.write_bytes(b"same sound")
            opts = {"model": "small", "device": "cpu", "prompt": "网络", "language": "zh"}
            original = worker.checkpoint_key(audio, opts)
            self.assertEqual(original, worker.checkpoint_key(audio, dict(opts, job_id="new-job")))
            self.assertNotEqual(original, worker.checkpoint_key(audio, dict(opts, model="base")))
            self.assertNotEqual(original, worker.checkpoint_key(audio, dict(opts, prompt="biology")))
            audio.write_bytes(b"changed sound")
            self.assertNotEqual(original, worker.checkpoint_key(audio, opts))

    def test_chunk_plan_covers_final_short_part_without_gaps(self):
        self.assertEqual(worker.plan_chunks(356.608, 300), [(0.0, 300.0), (300.0, 356.608)])
        self.assertEqual(worker.plan_chunks(300, 300), [(0.0, 300.0)])
        with self.assertRaises(ValueError):
            worker.plan_chunks(0, 300)

    def test_segments_have_stable_ids_global_timing_and_no_empty_captions(self):
        raw = [{"start": -0.02, "end": 1.2, "text": "  计算机网络  "},
               {"start": 1.2, "end": 9, "text": "协议"},
               {"start": 2, "end": 3, "text": " "}]
        parsed = worker.normalize_segments(raw, 300000, 302000, "key-1")
        self.assertEqual([s["text"] for s in parsed], ["计算机网络", "协议"])
        self.assertEqual(parsed[0]["start_ms"], 300000)
        self.assertEqual(parsed[-1]["end_ms"], 302000)
        self.assertEqual(parsed, worker.normalize_segments(raw, 300000, 302000, "key-1"))

    def test_protocol_version_and_device_are_validated_before_loading_model(self):
        with self.assertRaises(ValueError):
            worker.validate_request({"protocol_version": 999, "command": "probe"})
        with self.assertRaises(ValueError):
            worker.validate_request({"protocol_version": 1, "command": "transcribe", "device": "shell:bad"})
        worker.validate_request({"protocol_version": 1, "command": "probe"})


if __name__ == "__main__":
    unittest.main()
