"""Mock-engine tests: no model, GPU execution or network requests."""
import importlib
import io
import json
import os
from pathlib import Path
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch
import wave

from test_worker import worker


def request(**overrides):
    return dict(protocol_version=1, command="transcribe", model="small", device="cpu",
                model_dir="models", audio_path="audio.wav", checkpoint_dir="checkpoints",
                **overrides)


class EngineRequestTests(unittest.TestCase):
    def test_invalid_new_options_rejected_before_import(self):
        invalid = [{"engine": "remote"}, {"compute_type": "bf16"}, {"beam_size": 0},
                   {"beam_size": True}, {"best_of": 6}, {"best_of": None}, {"temperature": []},
                   {"temperature": [0, float("nan")]}, {"temperature": True},
                   {"condition_on_previous_text": "no"}, {"chunk_strategy": "vad-skip"},
                   {"context_ms": 10001}, {"decoding_policy": 42},
                   {"compute_type": "int8"}, {"compute_type": "float16"}]
        for values in invalid:
            with self.subTest(values=values), self.assertRaises(ValueError):
                worker.validate_request(request(**values))

    def test_legacy_defaults_preserve_temperature_fallback(self):
        self.assertTrue(hasattr(worker, "request_options"))
        options = worker.request_options(request())
        self.assertEqual(options["engine"], "openai-whisper")
        self.assertEqual(options["temperature"], [0.0, 0.2, 0.4, 0.6, 0.8, 1.0])
        self.assertIsNone(options["best_of"])
        self.assertEqual(options["beam_size"], 1)
        self.assertTrue(options["condition_on_previous_text"])
        worker.validate_request(request())

    def test_checkpoint_identity_binds_engine_runtime_model_planner_and_decode(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "audio.wav"
            audio.write_bytes(b"audio")
            base = request()
            original = worker.checkpoint_key(audio, base)
            for values in ({"engine": "faster-whisper"}, {"compute_type": "float32"},
                           {"model_sha256": "changed"}, {"runtime_versions": {"numpy": "new"}},
                           {"manifest_sha256": "changed"}, {"planner_version": "v2"},
                           {"context_ms": 1000}, {"beam_size": 2}, {"temperature": 0.0},
                           {"best_of": 1}, {"condition_on_previous_text": False},
                           {"decoding_policy": "local-recheck-v2"}):
                with self.subTest(values=values):
                    self.assertNotEqual(original, worker.checkpoint_key(audio, dict(base, **values)))


class FasterEngineTests(unittest.TestCase):
    def engine_module(self):
        self.assertTrue((Path(worker.__file__).parent / "engines" / "faster.py").is_file(),
                        "faster-whisper adapter is required")
        return importlib.import_module("engines.faster")

    def modules(self, factory=None):
        return {"faster_whisper": SimpleNamespace(__version__="mock-fw", WhisperModel=factory),
                "ctranslate2": SimpleNamespace(__version__="mock-ct2", get_cuda_device_count=lambda: 0,
                    get_supported_compute_types=lambda device: {"float32", "int8"}),
                "numpy": SimpleNamespace(__version__="mock-np")}

    def create_local_model(self, path):
        path.mkdir(parents=True, exist_ok=True)
        (path / "model.bin").write_bytes(b"mock model")
        for name in ("config.json", "tokenizer.json", "vocabulary.json"):
            (path / name).write_text("{}", encoding="utf-8")

    def test_lazy_generator_is_exhausted_and_defaults_explicit(self):
        module = self.engine_module()
        events, passed = [], {}
        def generate():
            events.append("inference-started")
            yield SimpleNamespace(start=0, end=1, text="你好", avg_logprob=-0.3,
                                  no_speech_prob=0.1, compression_ratio=1.2, words=None)
            events.append("inference-finished")
        model = SimpleNamespace(transcribe=lambda audio, **kwargs:
                                (passed.update(kwargs) or generate(), SimpleNamespace(language="zh")))
        with patch.dict(sys.modules, self.modules()):
            engine = module.FasterEngine(request(engine="faster-whisper"))
            result = engine.transcribe(model, object())
        self.assertEqual(events, ["inference-started", "inference-finished"])
        self.assertEqual(result["segments"][0]["text"], "你好")
        self.assertFalse(passed["vad_filter"])
        self.assertEqual(passed["beam_size"], 1)
        self.assertEqual(passed["best_of"], 5)
        self.assertEqual(passed["temperature"], [0, .2, .4, .6, .8, 1])
        self.assertIsNone(engine.metrics()["peak_gpu_mb"])
        self.assertIn("unavailable", engine.metrics()["gpu_metric"])

    def test_model_loading_is_local_only_and_single_worker(self):
        module = self.engine_module()
        factory = Mock(return_value="loaded")
        with tempfile.TemporaryDirectory() as temp, patch.dict(sys.modules, self.modules(factory)):
            model_dir = Path(temp) / "small"
            self.create_local_model(model_dir)
            values = dict(request(engine="faster-whisper"), model_dir=temp)
            engine = module.FasterEngine(values)
            path, checksum = engine.ensure_model(values, worker.Protocol(io.StringIO()))
            self.assertEqual(len(checksum), 64)
            self.assertEqual(engine.load_model(path), "loaded")
            self.assertTrue(factory.call_args.kwargs["local_files_only"])
            self.assertEqual(factory.call_args.kwargs["num_workers"], 1)
            self.assertEqual(factory.call_args.args[0], str(model_dir))

    def test_missing_or_incomplete_model_cannot_trigger_network_during_transcribe(self):
        module = self.engine_module()
        with tempfile.TemporaryDirectory() as temp, patch.dict(sys.modules, self.modules()):
            values = dict(request(engine="faster-whisper", allow_download=True), model_dir=temp)
            engine = module.FasterEngine(values)
            with self.assertRaises(FileNotFoundError):
                engine.ensure_model(values, worker.Protocol(io.StringIO()))
            model_dir = Path(temp) / "small"
            self.create_local_model(model_dir)
            (model_dir / "tokenizer.json").unlink()
            with self.assertRaises(FileNotFoundError):
                engine.ensure_model(values, worker.Protocol(io.StringIO()))

    def test_model_hash_includes_tokenizer_config_and_vocabulary(self):
        module = self.engine_module()
        with tempfile.TemporaryDirectory() as temp:
            model_dir = Path(temp) / "small"
            self.create_local_model(model_dir)
            previous = module.model_identity(model_dir)
            for name in ("model.bin", "config.json", "tokenizer.json", "vocabulary.json"):
                (model_dir / name).write_text('{"revision":2}', encoding="utf-8")
                new_hash = module.model_identity(model_dir)
                self.assertNotEqual(previous, new_hash)
                previous = new_hash
            (model_dir / "partial.incomplete").touch()
            with self.assertRaises(FileNotFoundError):
                module.model_identity(model_dir)

    def test_runtime_compute_support_is_enforced(self):
        module = self.engine_module()
        with patch.dict(sys.modules, self.modules()):
            with self.assertRaisesRegex(ValueError, "float16"):
                module.FasterEngine(request(engine="faster-whisper", compute_type="float16"))

    def test_probe_is_engine_specific_and_does_not_claim_cuda_inference_success(self):
        module = self.engine_module()
        modules = self.modules()
        modules["torch"] = None  # A future routing regression must not touch real hardware.
        modules["ctranslate2"].get_cuda_device_count = lambda: 1
        with patch.dict(sys.modules, modules), patch.object(worker.importlib.util, "find_spec", return_value=object()):
            info = worker.probe({"engine": "faster-whisper"})
        self.assertEqual(info.get("engine"), "faster-whisper")
        self.assertIn("ctranslate2", info["dependencies"])
        self.assertNotIn("torch", info["dependencies"])
        self.assertTrue(info["cuda_available"])
        self.assertEqual(info["supported_compute_types"]["cpu"], ["float32", "int8"])
        self.assertTrue(any("实际" in warning for warning in info["warnings"]))

    def test_probe_missing_dependency_returns_actionable_warning(self):
        self.engine_module()
        with patch.object(worker.importlib.util, "find_spec", return_value=None):
            info = worker.probe({"engine": "faster-whisper"})
        self.assertFalse(info["cuda_available"])
        self.assertIn("ctranslate2", info["dependencies"])
        self.assertTrue(any("ctranslate2" in warning for warning in info["warnings"]))

    def test_explicit_download_publishes_only_complete_official_model(self):
        module = self.engine_module()
        calls = []
        # huggingface-hub 1.x removed local_dir_use_symlinks. A permissive
        # **kwargs mock would hide this real download compatibility failure.
        def snapshot(repo_id, *, local_dir, local_files_only, allow_patterns):
            calls.append((repo_id, {"local_dir": local_dir,
                                   "local_files_only": local_files_only,
                                   "allow_patterns": allow_patterns}))
            self.create_local_model(Path(local_dir))
        with tempfile.TemporaryDirectory() as temp, patch.dict(sys.modules, {
                "huggingface_hub": SimpleNamespace(snapshot_download=snapshot)}):
            values = dict(request(engine="faster-whisper"), command="download_model", model_dir=temp)
            with patch.object(worker, "get_engine", side_effect=AssertionError("download must not initialize compute")):
                try:
                    path, digest = worker.ensure_model(values, worker.Protocol(io.StringIO()))
                except TypeError as error:
                    self.fail(f"Download must use supported huggingface-hub arguments: {error}")
            self.assertEqual(path, Path(temp) / "small")
            self.assertEqual(calls[0][0], "Systran/faster-whisper-small")
            self.assertEqual(module.model_identity(path), digest)
            self.assertFalse(calls[0][1]["local_files_only"])

    def test_incomplete_download_cannot_become_a_loadable_model(self):
        self.engine_module()
        def snapshot(repo_id, **kwargs):
            (Path(kwargs["local_dir"]) / "model.bin").write_bytes(b"partial")
        with tempfile.TemporaryDirectory() as temp, patch.dict(sys.modules, {
                "huggingface_hub": SimpleNamespace(snapshot_download=snapshot)}):
            values = dict(request(engine="faster-whisper"), command="download_model", model_dir=temp)
            with self.assertRaises(FileNotFoundError):
                worker.ensure_model(values, worker.Protocol(io.StringIO()))
            self.assertFalse((Path(temp) / "small").exists())

    def test_local_cuda_directory_is_registered_before_ct2_import(self):
        module = self.engine_module()
        events = []
        modules = self.modules()
        def import_module(name):
            events.append(name)
            return modules[name]
        with patch.object(module, "register_local_cuda", side_effect=lambda: events.append("dll")), \
                patch.object(module.importlib, "import_module", side_effect=import_module):
            module.FasterEngine(request(engine="faster-whisper"))
        self.assertEqual(events[:2], ["dll", "ctranslate2"])


class OpenAIEngineTests(unittest.TestCase):
    def test_decoding_defaults_and_cuda_synchronization_use_original_torch_only(self):
        module = importlib.import_module("engines.openai")
        torch = SimpleNamespace(__version__="mock", version=SimpleNamespace(cuda="11.8"),
                                cuda=Mock(), set_num_threads=Mock())
        whisper = SimpleNamespace(__version__="mock", load_model=Mock())
        model = Mock()
        with patch.dict(sys.modules, {"torch": torch, "whisper": whisper}):
            engine = module.OpenAIEngine(dict(request(), device="cuda"))
            engine.synchronize()
            engine.transcribe(model, object())
        torch.cuda.synchronize.assert_called_once()
        self.assertTrue(model.transcribe.call_args.kwargs["fp16"])
        self.assertEqual(model.transcribe.call_args.kwargs["temperature"], [0, .2, .4, .6, .8, 1])
        self.assertIsNone(model.transcribe.call_args.kwargs["best_of"])
        self.assertIsNone(model.transcribe.call_args.kwargs["beam_size"])
        self.assertIsNone(engine.decode_options["beam_size"])
        self.assertIsNone(engine.decode_options["best_of"])
        self.assertEqual(engine.compute_type, "float16")
        self.assertEqual(model.transcribe.call_args.kwargs, {
            "language": None, "initial_prompt": None, "fp16": True, "verbose": False,
            "condition_on_previous_text": True, "beam_size": None, "best_of": None,
            "temperature": [0, .2, .4, .6, .8, 1], "word_timestamps": False,
            "compression_ratio_threshold": 2.4, "logprob_threshold": -1.0,
            "no_speech_threshold": .6})

    def test_explicit_decode_overrides_are_preserved_and_recorded(self):
        module = importlib.import_module("engines.openai")
        torch = SimpleNamespace(__version__="mock", version=SimpleNamespace(cuda="11.8"),
                                cuda=Mock(), set_num_threads=Mock())
        whisper = SimpleNamespace(__version__="mock", load_model=Mock())
        for beam, effective_beam in ((1, None), (3, 3)):
            with self.subTest(beam=beam), patch.dict(sys.modules, {"torch": torch, "whisper": whisper}):
                values = request(beam_size=beam, best_of=1, temperature=0)
                worker.validate_request(values)
                engine = module.OpenAIEngine(values)
                model = Mock()
                engine.transcribe(model, object())
                self.assertEqual(model.transcribe.call_args.kwargs["beam_size"], effective_beam)
                self.assertEqual(model.transcribe.call_args.kwargs["best_of"], 1)
                self.assertEqual(engine.decode_options["beam_size"], effective_beam)
                self.assertEqual(engine.decode_options["best_of"], 1)
                self.assertEqual(engine.decode_options["temperature"], 0)


class MockEngine:
    engine_version = "mock-1"
    runtime_versions = {"python": "mock", "engine": "mock-1"}
    compute_type = "float32"
    warnings = []

    def __init__(self):
        self.loads = self.inferences = self.syncs = 0
        self.decode_options = {"beam_size": 1, "best_of": 5, "temperature": [0, .2, .4, .6, .8, 1],
                               "condition_on_previous_text": True}

    def ensure_model(self, values, protocol):
        return Path("mock-model"), "model-hash"

    def load_model(self, path):
        self.loads += 1
        return object()

    def transcribe(self, model, audio):
        self.inferences += 1
        return {"segments": [{"start": 0, "end": 0.5, "text": "课程"}]}

    def synchronize(self):
        self.syncs += 1

    def metrics(self):
        return {"peak_gpu_mb": None, "peak_gpu_reserved_mb": None, "gpu_metric": "mock-unavailable"}


class WorkerTimingAndRecoveryTests(unittest.TestCase):
    def test_half_millisecond_audio_duration_matches_rust_rounding(self):
        with tempfile.TemporaryDirectory() as temp:
            values = self.fixture(temp, seconds=1)
            with wave.open(values["audio_path"], "wb") as stream:
                stream.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
                stream.writeframes(b"\0\0" * 16008)
            with patch.object(worker, "get_engine", return_value=MockEngine()), \
                    patch.dict(sys.modules, {"numpy": self.fake_numpy()}):
                result = worker.transcribe(values, worker.Protocol(io.StringIO()))
            self.assertEqual(result["duration_ms"], 1001)
            self.assertEqual(result["manifest"][-1]["end_ms"], 1001)
            self.assertEqual(result["manifest"][-1]["decode_end_ms"], 1001)

    def fixture(self, temp, seconds=61):
        audio = Path(temp) / "audio.wav"
        with wave.open(str(audio), "wb") as stream:
            stream.setparams((1, 2, 16000, 0, "NONE", "not compressed"))
            stream.writeframes(b"\0\0" * (seconds * 16000))
        return dict(request(chunk_seconds=30), audio_path=str(audio), checkpoint_dir=temp)

    def fake_numpy(self):
        array = Mock()
        array.astype.return_value.__truediv__ = Mock(return_value=array)
        return SimpleNamespace(frombuffer=lambda *args: array, int16="int16", float32="float32")

    def test_truncated_pcm_cannot_be_marked_complete(self):
        with tempfile.TemporaryDirectory() as temp:
            values = self.fixture(temp, seconds=1)
            audio = Path(values["audio_path"])
            audio.write_bytes(audio.read_bytes()[:-100])
            with patch.object(worker, "get_engine", return_value=MockEngine()), \
                    patch.dict(sys.modules, {"numpy": self.fake_numpy()}):
                with self.assertRaisesRegex(ValueError, "截断"):
                    worker.transcribe(values, worker.Protocol(io.StringIO()))

    def test_interrupted_later_chunk_preserves_previous_checkpoint_for_resume(self):
        with tempfile.TemporaryDirectory() as temp:
            values = self.fixture(temp)
            engine = MockEngine()
            calls = []
            def interrupted(model, audio):
                calls.append(1)
                if len(calls) == 2:
                    raise RuntimeError("simulated cancellation")
                return {"segments": [{"start": 0, "end": .5, "text": "课程"}]}
            with patch.object(worker, "get_engine", return_value=engine), \
                    patch.dict(sys.modules, {"numpy": self.fake_numpy()}):
                with patch.object(engine, "transcribe", side_effect=interrupted):
                    with self.assertRaisesRegex(RuntimeError, "simulated cancellation"):
                        worker.transcribe(values, worker.Protocol(io.StringIO()))
                self.assertEqual(len(list(Path(temp).glob("*/000000.json"))), 1)
                self.assertEqual(list(Path(temp).glob("*/000001.json")), [])
                result = worker.transcribe(values, worker.Protocol(io.StringIO()))
                self.assertEqual(result["resumed_chunks"], 1)
                self.assertEqual(result["completed_chunk_indices"], [0, 1, 2])

    def test_experimental_missing_word_timing_leaves_no_completed_chunk(self):
        with tempfile.TemporaryDirectory() as temp:
            values = dict(self.fixture(temp, 1), chunk_strategy="speech-boundary")
            with patch.object(worker, "get_engine", return_value=MockEngine()), \
                    patch.dict(sys.modules, {"numpy": self.fake_numpy()}):
                with self.assertRaisesRegex(ValueError, "词级时间戳"):
                    worker.transcribe(values, worker.Protocol(io.StringIO()))
            self.assertEqual(list(Path(temp).glob("*/000000.json")), [])

    def test_ambiguous_second_boundary_is_rejected_again_after_resume(self):
        with tempfile.TemporaryDirectory() as temp:
            values = dict(self.fixture(temp), chunk_strategy="speech-boundary",
                          speech_intervals_ms=[[0, 61000]])
            engine = MockEngine()
            calls = []
            def decode(model, audio):
                # First core has words far from its boundary. The second decode
                # places a word in the shared context at absolute 29.9–30.5s.
                start, end = (.1, .2) if not calls else (1.9, 2.5)
                calls.append(1)
                return {"segments": [{"start": start, "end": end, "text": "课程", "words": [
                    {"start": start, "end": end, "word": "课程"}]}]}
            with patch.object(worker, "get_engine", return_value=engine), \
                    patch.dict(sys.modules, {"numpy": self.fake_numpy()}), \
                    patch.object(engine, "transcribe", side_effect=decode):
                for attempt in range(2):
                    with self.subTest(attempt=attempt), self.assertRaisesRegex(ValueError, "边界存在讲话/对齐歧义"):
                        worker.transcribe(values, worker.Protocol(io.StringIO()))
                    self.assertEqual(len(list(Path(temp).glob("*/000000.json"))), 1)
                    self.assertEqual(list(Path(temp).glob("*/000001.json")), [])
                    self.assertEqual(list(Path(temp).glob("*/000002.json")), [])
            self.assertEqual(len(calls), 3)  # The verified first checkpoint was reused.
            manifest = json.loads(next(Path(temp).glob("*/manifest.json")).read_text(encoding="utf-8"))
            self.assertEqual(manifest["identity"]["planner_version"], "speech-boundary-v2-silent-context")
            self.assertEqual(manifest["identity"]["normalizer_version"], 3)

    def test_full_resume_skips_load_and_corrupt_or_missing_chunks_are_redecoded(self):
        self.assertTrue(hasattr(worker, "get_engine"), "worker must use the engine interface")
        with tempfile.TemporaryDirectory() as temp:
            values = self.fixture(temp)
            audio = Path(values["audio_path"])
            numpy = self.fake_numpy()
            engine = MockEngine()
            with patch.object(worker, "get_engine", return_value=engine), patch.dict(sys.modules, {"numpy": numpy}):
                first = worker.transcribe(values, worker.Protocol(io.StringIO()))
                self.assertEqual(first["completed_chunk_indices"], [0, 1, 2])
                self.assertTrue(first["model_loaded"])
                self.assertEqual(engine.loads, 1)
                self.assertEqual(engine.inferences, 3)
                self.assertEqual(first["audio_sha256"], worker.file_sha256(audio))
                self.assertEqual(first["manifest"][-1]["end_ms"], 61000)
                second = worker.transcribe(values, worker.Protocol(io.StringIO()))
                self.assertFalse(second["model_loaded"])
                self.assertEqual(second["resumed_chunks"], 3)
                self.assertEqual(second["timings"]["model_load_seconds"], 0)
                self.assertEqual(second["timings"]["inference_seconds"], 0)
                self.assertEqual(engine.loads, 1)
                checkpoint_dir = Path(temp) / first["checkpoint_key"]
                (checkpoint_dir / "000000.json").unlink()
                (checkpoint_dir / "000001.json").write_text("{broken", encoding="utf-8")
                third = worker.transcribe(values, worker.Protocol(io.StringIO()))
                self.assertEqual(third["resumed_chunks"], 1)
                self.assertEqual(engine.inferences, 5)
                for key in ("dependency_seconds", "model_verify_seconds", "audio_hash_seconds",
                            "model_load_seconds", "inference_seconds", "checkpoint_seconds", "total_seconds"):
                    self.assertGreaterEqual(third["timings"][key], 0)


if __name__ == "__main__":
    unittest.main()
