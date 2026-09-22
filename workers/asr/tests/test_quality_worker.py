"""Standard-library tests; detector inference is mocked, not a model accuracy test."""
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch
import wave

sys.path.insert(0, str(Path(__file__).parents[1]))
SPEC = importlib.util.spec_from_file_location("quality_worker", Path(__file__).parents[1] / "quality_worker.py")
quality = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(quality)


def make_wav(path, frames=1600, rate=16000):
    with wave.open(str(path), "wb") as stream:
        stream.setnchannels(1)
        stream.setsampwidth(2)
        stream.setframerate(rate)
        stream.writeframes(b"\0\0" * frames)


class QualityTests(unittest.TestCase):
    def setUp(self):
        identity = patch.object(quality, "detector_identity", return_value="mock-identity", create=True)
        identity.start()
        self.addCleanup(identity.stop)

    def test_intervals_are_clamped_and_truncated_wav_is_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "a.wav"
            make_wav(audio)
            events = iter([{"start": -300}, {"end": 99999}, None, None])
            with patch.object(quality, "load_detector", return_value=(lambda pcm: next(events), "mock")):
                self.assertEqual(quality.detect_speech({"audio_path": str(audio)})["speech"],
                                 [{"start_ms": 0, "end_ms": 100}])
            audio.write_bytes(audio.read_bytes()[:-100])
            with patch.object(quality, "load_detector", return_value=(lambda pcm: None, "mock")):
                with self.assertRaisesRegex(ValueError, "不完整"):
                    quality.detect_speech({"audio_path": str(audio)})

    def test_streaming_closes_unfinished_speech_at_eof_and_pads_final_window(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "a.wav"
            make_wav(audio)
            calls = []
            def detector(pcm):
                calls.append(len(pcm))
                return {"start": -50} if len(calls) == 1 else None
            with patch.object(quality, "load_detector", return_value=(detector, "mock-v1")):
                result = quality.detect_speech({"audio_path": str(audio)})
            self.assertEqual(calls, [1024] * 4)
            self.assertEqual(result["speech"], [{"start_ms": 0, "end_ms": 100}])
            self.assertEqual(result["audio_sha256"], quality.file_sha256(audio))
            self.assertEqual(result["detector_version"], "mock-v1")
            self.assertEqual(result["detector_identity"], "mock-identity")

    def test_silence_and_invalid_format(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "a.wav"
            make_wav(audio)
            with patch.object(quality, "load_detector", return_value=(lambda pcm: None, "mock")):
                self.assertEqual(quality.detect_speech({"audio_path": str(audio)})["speech"], [])
            make_wav(audio, rate=8000)
            with patch.object(quality, "load_detector") as load:
                with self.assertRaises(ValueError):
                    quality.detect_speech({"audio_path": str(audio)})
                load.assert_not_called()

    def test_invalid_model_intervals_and_changed_audio_are_rejected(self):
        with tempfile.TemporaryDirectory() as temp:
            audio = Path(temp) / "a.wav"
            make_wav(audio)
            with patch.object(quality, "load_detector", return_value=(lambda pcm: {"start": float("nan")}, "mock")):
                with self.assertRaises(ValueError):
                    quality.detect_speech({"audio_path": str(audio)})
            with patch.object(quality, "load_detector", return_value=(lambda pcm: None, "mock")), patch.object(quality, "file_sha256", side_effect=["before", "after"]):
                with self.assertRaisesRegex(RuntimeError, "音频"):
                    quality.detect_speech({"audio_path": str(audio)})

    def test_protocol_errors_are_jsonl_without_heavy_dependencies(self):
        for request in ({"protocol_version": 1, "job_id": "j", "command": "bad"}, []):
            run = subprocess.run([sys.executable, str(Path(__file__).parents[1] / "quality_worker.py")],
                                 input=json.dumps(request) + "\n", capture_output=True, text=True, encoding="utf-8", timeout=15)
            self.assertEqual(run.returncode, 1)
            event = json.loads(run.stdout)
            self.assertEqual(event["code"], "invalid_request")


class DetectorIdentityTests(unittest.TestCase):
    def test_identity_subprocess_does_not_import_model_packages(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            for name in ("silero_vad", "torch", "torchaudio", "numpy"):
                package = directory / name
                package.mkdir()
                (package / "__init__.py").write_text("raise RuntimeError('must not import model packages')\n")
                metadata = directory / f"{name}-1.0.dist-info"
                metadata.mkdir()
                (metadata / "METADATA").write_text(f"Metadata-Version: 2.1\nName: {name.replace('_', '-')}\nVersion: 1.0\n")
            (directory / "silero_vad" / "data").mkdir()
            (directory / "silero_vad" / "data" / "silero_vad.jit").write_bytes(b"mock model")
            run = subprocess.run([sys.executable, str(Path(__file__).parents[1] / "quality_worker.py")],
                                 input=json.dumps({"protocol_version": 1, "job_id": "identity", "command": "detector_identity"}) + "\n",
                                 capture_output=True, text=True, encoding="utf-8", timeout=15,
                                 env=dict(os.environ, PYTHONPATH=str(directory)))
            self.assertEqual(run.returncode, 0, run.stderr)
            events = [json.loads(line) for line in run.stdout.splitlines()]
            self.assertEqual([event["type"] for event in events], ["hello", "done"])
            self.assertEqual(len(events[-1]["detector_identity"]), 64)

    def test_identity_requires_no_audio(self):
        quality.validate_request({"protocol_version": 1, "job_id": "j", "command": "detector_identity"})

    def test_identity_is_stable_and_changes_with_model_runtime_options_and_worker(self):
        with tempfile.TemporaryDirectory() as temp:
            directory = Path(temp)
            package = directory / "silero_vad"
            (package / "data").mkdir(parents=True)
            model = package / "data" / "silero_vad.jit"
            model.write_bytes(b"model")
            (directory / "quality_worker.py").write_bytes(b"quality")
            (directory / "worker.py").write_bytes(b"worker")
            spec = SimpleNamespace(submodule_search_locations=[str(package)])
            with patch.object(quality.importlib.util, "find_spec", return_value=spec), \
                    patch.object(quality.importlib.metadata, "version", return_value="1.0"), \
                    patch.object(quality, "__file__", str(directory / "quality_worker.py")):
                original = quality.detector_identity()
                self.assertEqual(original, quality.detector_identity())
                self.assertEqual(len(original), 64)
                model.write_bytes(b"other model")
                self.assertNotEqual(original, quality.detector_identity())
                model.write_bytes(b"model")
                with patch.object(quality.sys, "executable", "another-python.exe"):
                    self.assertNotEqual(original, quality.detector_identity())
                with patch.dict(quality.DETECTOR_OPTIONS, threshold=0.6):
                    self.assertNotEqual(original, quality.detector_identity())
                with patch.object(quality.importlib.metadata, "version", return_value="2.0"):
                    self.assertNotEqual(original, quality.detector_identity())
                (directory / "worker.py").write_bytes(b"changed worker")
                self.assertNotEqual(original, quality.detector_identity())

    def test_missing_detector_fails_without_loading_model(self):
        with patch.object(quality.importlib.util, "find_spec", return_value=None):
            with self.assertRaises(ImportError):
                quality.detector_identity()


if __name__ == "__main__":
    unittest.main()
