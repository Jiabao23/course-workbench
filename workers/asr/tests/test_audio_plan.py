"""Deterministic planner and ownership tests; no ASR or hardware is used."""
import unittest
import inspect
from test_worker import worker


class AudioPlanTests(unittest.TestCase):
    def test_timestamp_half_milliseconds_round_up(self):
        parsed = worker.normalize_segments([{"start": .0005, "end": 1.0005, "text": "词"}],
                                           0, 2000, "key")
        self.assertEqual((parsed[0]["start_ms"], parsed[0]["end_ms"]), (1, 1001))
        manifest = self.planner(100000, chunk_seconds=30.0005)
        self.assertEqual(manifest[0]["end_ms"], 30001)

    def planner(self, *args, **kwargs):
        self.assertTrue(hasattr(worker, "build_manifest"), "versioned manifest planner is required")
        return worker.build_manifest(*args, **kwargs)

    def test_fixed_manifest_covers_subsecond_tail(self):
        manifest = self.planner(600001)
        self.assertEqual([(c["start_ms"], c["end_ms"]) for c in manifest],
                         [(0, 300000), (300000, 600000), (600000, 600001)])
        self.assertTrue(all(c["start_ms"] == c["decode_start_ms"] for c in manifest))

    def test_pause_is_preferred_and_context_never_changes_core_coverage(self):
        manifest = self.planner(610250, strategy="speech-boundary",
                                speech_intervals_ms=[[0, 298000], [302000, 610250]])
        self.assertEqual(manifest[0]["end_ms"], 300000)
        self.assertEqual(manifest[0]["decode_end_ms"], 302000)
        self.assertEqual(manifest[1]["decode_start_ms"], 298000)
        self.assertEqual(manifest[-1]["end_ms"], 610250)
        for a, b in zip(manifest, manifest[1:]):
            self.assertEqual(a["end_ms"], b["start_ms"])

    def test_nearby_pause_can_move_target_and_continuous_speech_is_bounded(self):
        shifted = self.planner(700000, strategy="speech-boundary",
                               speech_intervals_ms=[[0, 310000], [312000, 700000]])
        self.assertEqual(shifted[0]["end_ms"], 311000)
        continuous = self.planner(900001, strategy="speech-boundary",
                                  speech_intervals_ms=[[0, 900001]])
        self.assertEqual(continuous[-1]["end_ms"], 900001)
        self.assertTrue(all(0 < c["end_ms"] - c["start_ms"] <= 360000 for c in continuous))

    def test_unsorted_overlapping_evidence_is_merged_without_skipping_gaps(self):
        first = self.planner(610000, strategy="speech-boundary",
                             speech_intervals_ms=[[302000, 610000], [0, 200000], [190000, 298000]])
        second = self.planner(610000, strategy="speech-boundary",
                              speech_intervals_ms=[[0, 298000], [302000, 610000]])
        self.assertEqual(first, second)
        self.assertEqual(first[0]["start_ms"], 0)

    def test_invalid_intervals_are_not_silently_treated_as_silence(self):
        for intervals in ([[20, 10]], [[-1, 100]], [[0, 700001]], [[True, 100]], ["bad"]):
            with self.subTest(intervals=intervals), self.assertRaises(ValueError):
                self.planner(700000, strategy="speech-boundary", speech_intervals_ms=intervals)

    def test_word_midpoint_ownership_preserves_actual_repetitions(self):
        self.assertTrue(hasattr(worker, "normalize_owned_segments"), "word ownership is required")
        raw = [{"start": 0, "end": 4, "text": "yes yes yes", "words": [
            {"start": 0.0, "end": 1.0, "word": "yes"},
            {"start": 1.0, "end": 3.0, "word": " yes"},
            {"start": 3.0, "end": 4.0, "word": " yes"}]}]
        left = worker.normalize_owned_segments(raw, 0, 0, 2000, "left")
        right = worker.normalize_owned_segments(raw, 0, 2000, 4000, "right")
        self.assertEqual([s["text"] for s in left + right], ["yes", "yes yes"])
        self.assertEqual(right[0]["start_ms"], 2000)
        self.assertEqual(right[-1]["end_ms"], 4000)

    def test_missing_incomplete_or_ambiguous_word_timestamps_fail_experimental(self):
        self.assertTrue(hasattr(worker, "normalize_owned_segments"), "word ownership is required")
        for raw in ([{"start": 0, "end": 4, "text": "讲解"}],
                    [{"start": 0, "end": 4, "text": "讲解", "words": []}],
                    [{"start": 0, "end": 4, "text": "讲解", "words": [
                        {"start": 0, "end": 2, "word": "讲"}]}],
                    [{"start": 0, "end": 4, "text": "讲解", "words": [
                        {"start": 1, "end": 1, "word": "讲解"}]}]):
            with self.subTest(raw=raw), self.assertRaisesRegex(ValueError, "词级时间戳"):
                worker.normalize_owned_segments(raw, 0, 0, 2000, "key")

    def test_out_of_decode_range_words_are_ambiguous_not_silently_discarded(self):
        self.assertIn("decode_end_ms", inspect.signature(worker.normalize_owned_segments).parameters)
        for start, end in ((-1, -.5), (5, 6)):
            raw = [{"start": start, "end": end, "text": "词", "words": [
                {"start": start, "end": end, "word": "词"}]}]
            with self.subTest(start=start), self.assertRaisesRegex(ValueError, "词级时间戳"):
                worker.normalize_owned_segments(raw, 0, 0, 2000, "key", decode_end_ms=4000)

    def test_independent_timestamp_drift_cannot_silently_lose_or_duplicate_boundary_word(self):
        self.assertTrue(hasattr(worker, "require_silent_boundary_context"))
        # The same true word drifts to opposite sides of the ownership midpoint.
        # Forward drift loses both copies; reverse drift would retain both copies.
        for left_times, right_times in (((1.9, 2.5), (.7, 1.1)), ((1.7, 2.1), (.9, 1.5))):
            for start, end, offset, chunk in (
                    (*left_times, 0, {"start_ms": 0, "end_ms": 2000}),
                    (*right_times, 1000, {"start_ms": 2000, "end_ms": 4000})):
                segments = [{"start": start, "end": end, "text": "课程", "words": [
                    {"start": start, "end": end, "word": "课程"}]}]
                with self.subTest(times=(start, end), chunk=chunk), \
                        self.assertRaisesRegex(ValueError, "边界存在讲话/对齐歧义"):
                    worker.require_silent_boundary_context(segments, offset, chunk, 4000, 1000)

    def test_verified_silent_shared_context_preserves_repeated_words_away_from_boundary(self):
        self.assertTrue(hasattr(worker, "require_silent_boundary_context"))
        raw = [{"start": .1, "end": .8, "text": "yes yes", "words": [
            {"start": .1, "end": .3, "word": "yes"},
            {"start": .5, "end": .8, "word": " yes"}]}]
        worker.require_silent_boundary_context(raw, 0, {"start_ms": 0, "end_ms": 2000}, 4000, 1000)
        parsed = worker.normalize_owned_segments(raw, 0, 0, 2000, "key", decode_end_ms=3000)
        self.assertEqual(parsed[0]["text"], "yes yes")


if __name__ == "__main__":
    unittest.main()
