use course_core::quality_intervals::{overlaps_speech, uncovered_speech};

// Independent reference: the original per-speech scan, including its treatment
// of zero-length and malformed text timings. Keep this intentionally simple.
fn legacy_uncovered(speech: &[(u64, u64)], text: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut text = text.to_vec();
    text.sort_by_key(|&(start, _)| start);
    let mut gaps = Vec::new();
    for &(start, end) in speech {
        let mut cursor = start;
        for &(text_start, text_end) in &text {
            if text_end <= cursor {
                continue;
            }
            if text_start >= end {
                break;
            }
            let missing_end = text_start.min(end);
            if missing_end.saturating_sub(cursor) >= 1500 {
                gaps.push((cursor, missing_end));
            }
            cursor = cursor.max(text_end).min(end);
        }
        if end.saturating_sub(cursor) >= 1500 {
            gaps.push((cursor, end));
        }
    }
    gaps
}

#[test]
fn gaps_include_exactly_1500_ms_but_never_join_across_speech_boundaries() {
    let speech = [(0, 5000), (5000, 6499), (6500, 8000)];
    let text = [(1500, 2000), (3499, 3500)];
    assert_eq!(
        uncovered_speech(&speech, &text),
        vec![(0, 1500), (3500, 5000), (6500, 8000)]
    );
}

#[test]
fn unsorted_nested_overlapping_and_touching_text_is_union_coverage() {
    let text = [
        (9000, 11000),
        (2500, 4000),
        (1500, 3000),
        (4000, 7000),
        (5000, 6000),
    ];
    assert_eq!(
        uncovered_speech(&[(0, 13000)], &text),
        vec![(0, 1500), (7000, 9000), (11000, 13000)]
    );
}

#[test]
fn zero_length_text_retains_legacy_gap_boundaries() {
    assert_eq!(
        uncovered_speech(&[(0, 6000)], &[(3000, 3000)]),
        vec![(0, 3000), (3000, 6000)]
    );
    assert_eq!(
        uncovered_speech(&[(0, 2000)], &[(1000, 1000)]),
        Vec::<(u64, u64)>::new()
    );
    let text = [
        (0, 0),
        (1500, 1500),
        (1500, 4500),
        (3000, 3000),
        (6000, 6000),
    ];
    assert_eq!(
        uncovered_speech(&[(0, 6000)], &text),
        vec![(0, 1500), (4500, 6000)]
    );
}

#[test]
fn a_text_span_crossing_many_speech_intervals_stays_active() {
    let speech = [(0, 4000), (5000, 9000), (10000, 14000), (16000, 19000)];
    assert_eq!(
        uncovered_speech(&speech, &[(1500, 17000)]),
        vec![(0, 1500), (17000, 19000)]
    );
}

#[test]
fn empty_inputs_and_unsigned_time_limits_do_not_overflow() {
    assert!(uncovered_speech(&[], &[(0, 9000)]).is_empty());
    assert_eq!(uncovered_speech(&[(0, 3000)], &[]), vec![(0, 3000)]);
    let max = u64::MAX;
    assert_eq!(
        uncovered_speech(&[(max - 9000, max)], &[(max - 7500, max - 1500)]),
        vec![(max - 9000, max - 7500), (max - 1500, max)]
    );
}

#[test]
fn malformed_reversed_text_preserves_existing_evidence() {
    let speech = [(0, 10000), (12000, 16000)];
    let text = [(5000, 3000), (8000, 6000), (15000, 14000)];
    assert_eq!(
        uncovered_speech(&speech, &text),
        legacy_uncovered(&speech, &text)
    );
}

struct Generator(u64);
impl Generator {
    fn next(&mut self, bound: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 16) % bound
    }
}

#[test]
fn deterministic_random_timelines_match_the_original_scan() {
    let mut rng = Generator(0x6a09e667f3bcc909);
    for case in 0..5000 {
        let mut speech = Vec::new();
        let mut end = 0;
        for _ in 0..rng.next(36) {
            let start = end + rng.next(12) * 500;
            end = start + 1 + rng.next(40) * 500;
            speech.push((start, end));
        }
        let mut text = Vec::new();
        for _ in 0..rng.next(80) {
            let start = rng.next(end + 20000);
            let length = rng.next(16) * 500;
            text.push((start, start + length));
            if rng.next(5) == 0 {
                text.push((start, start));
            }
        }
        assert_eq!(
            uncovered_speech(&speech, &text),
            legacy_uncovered(&speech, &text),
            "case {case}, speech={speech:?}, text={text:?}"
        );
    }
}

#[test]
fn overlap_lookup_preserves_strict_touching_and_point_semantics() {
    let speech = [(1000, 3000), (3000, 4000), (6000, 8000)];
    for (start, end, expected) in [
        (0, 1000, false),
        (0, 1001, true),
        (4000, 6000, false),
        (8000, 9000, false),
        (2000, 2000, true),
        (3000, 3000, false),
        (2500, 1500, true),
        (8000, 6000, false),
    ] {
        assert_eq!(overlaps_speech(&speech, start, end), expected);
    }
    assert!(!overlaps_speech(&[], 0, 1000));
    let mut rng = Generator(0xbb67ae8584caa73b);
    for _ in 0..4000 {
        let start = rng.next(10000);
        let end = rng.next(10000);
        assert_eq!(
            overlaps_speech(&speech, start, end),
            speech.iter().any(|&(a, b)| a < end && b > start),
            "query {start}..{end}"
        );
    }
}
