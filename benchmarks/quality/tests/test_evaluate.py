import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("evaluate", Path(__file__).parents[1] / "evaluate.py")
e = importlib.util.module_from_spec(spec)
spec.loader.exec_module(e)


class MetricsTests(unittest.TestCase):
    def reference(self):
        return {"schema_version": 1, "audio_sha256": "a"*64, "reviewed": True,
                "reviewer": "fixture-author", "reviewed_at": "2026-10-04",
                "text": "计算机网络，TCP。", "terms": ["TCP"],
                "speech_intervals_ms": [[0, 1000], [2000, 3000]],
                "omission_intervals_ms": [[2000, 3000]]}

    def test_known_edit_counts_and_unicode_policy(self):
        self.assertEqual(e.edit_distance("abc", "axcd"), 2)
        self.assertEqual(e.normalize("Ａ， B!\n网络"), "ab网络")
        self.assertNotEqual(e.normalize("網絡"), e.normalize("网络"))

    def test_constructed_truth_cer_terms_and_overlapping_intervals(self):
        h = {"audio_sha256": "a"*64, "segments": [{"text": "计算机网路 TCP", "start_ms": 0, "end_ms": 1000}],
             "omission_intervals_ms": [[1500, 2500], [2000, 3000]]}
        result = e.evaluate(self.reference(), h)
        self.assertEqual(result["normalized_cer"], 1/8)
        self.assertEqual(result["term_counts"][0]["missing"], 0)
        self.assertEqual(result["speech_coverage"]["recall"], 0.5)
        self.assertAlmostEqual(result["omission_detection"]["precision"], 2/3)
        self.assertEqual(result["omission_detection"]["recall"], 1)

    def test_unreviewed_and_other_audio_rejected(self):
        r = self.reference()
        for key, value in [("reviewed", False), ("reviewer", ""), ("text", "")]:
            with self.assertRaises(ValueError): e.evaluate(dict(r, **{key:value}), {"audio_sha256":"a"*64,"segments":[]})
        with self.assertRaises(ValueError): e.evaluate(r, {"audio_sha256":"b"*64,"segments":[]})

    def test_empty_predictions_and_bad_intervals_are_not_false_success(self):
        result=e.evaluate(self.reference(), {"audio_sha256":"a"*64,"segments":[]})
        self.assertEqual(result["normalized_cer"], 1)
        self.assertEqual(result["speech_coverage"]["recall"], 0)
        self.assertIsNone(result["speech_coverage"]["precision"])
        for bad in [[[2,1]], [[-1,2]], [[0,float('nan')]], [[False,1]]]:
            with self.assertRaises(ValueError): e.interval_metrics(bad, [])


if __name__ == "__main__": unittest.main()
