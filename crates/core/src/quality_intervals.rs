//! Pure interval checks used by quality reports. Times are milliseconds.

const MIN_UNCOVERED_MS: u64 = 1500;

/// Return text-free portions of speech lasting at least 1500 ms, in speech order.
///
/// `speech` must contain positive, sorted, nonoverlapping intervals; callers
/// validate evidence before calling. Text may be unsorted, overlapping or zero
/// length. Zero-length text retains the original report's gap-splitting behavior.
/// Sorting/merging takes O(T log T); the difference sweep takes O(T + V + G),
/// where T is text count, V is speech count and G is returned gap count.
pub fn uncovered_speech(speech: &[(u64, u64)], text: &[(u64, u64)]) -> Vec<(u64, u64)> {
    if speech.is_empty() {
        return Vec::new();
    }
    let mut sorted = text.to_vec();
    sorted.sort_by_key(|&(start, _)| start);
    // Reversed timings are already structural faults in the report. Their old
    // behavior can emit overlapping gaps and cannot be represented by a union;
    // retain it for malformed legacy transcripts rather than alter review IDs.
    if sorted.iter().any(|&(start, end)| end < start) {
        return malformed_text_gaps(speech, &sorted);
    }
    let mut merged: Vec<(u64, u64)> = Vec::with_capacity(sorted.len());
    for (start, end) in sorted {
        if let Some(previous) = merged.last_mut() {
            if start <= previous.1 {
                previous.1 = previous.1.max(end);
                continue;
            }
        }
        merged.push((start, end));
    }
    let mut gaps = Vec::new();
    let mut index = 0;
    for &(start, end) in speech {
        let mut cursor = start;
        while index < merged.len() && merged[index].1 <= cursor {
            index += 1;
        }
        while let Some(&(text_start, text_end)) = merged.get(index) {
            if text_start >= end {
                break;
            }
            if text_start.saturating_sub(cursor) >= MIN_UNCOVERED_MS {
                gaps.push((cursor, text_start));
            }
            cursor = cursor.max(text_end).min(end);
            if text_end >= end {
                // This span may also cover later speech; keep it active.
                break;
            }
            index += 1;
        }
        if end.saturating_sub(cursor) >= MIN_UNCOVERED_MS {
            gaps.push((cursor, end));
        }
    }
    gaps
}

/// O(log V) strict overlap lookup for sorted, nonoverlapping speech intervals.
/// Touching endpoints do not overlap. The predicate matches `a < end && b > start`,
/// including point queries used by legacy reports.
pub fn overlaps_speech(speech: &[(u64, u64)], start: u64, end: u64) -> bool {
    let index = speech.partition_point(|&(_, speech_end)| speech_end <= start);
    speech
        .get(index)
        .is_some_and(|&(speech_start, _)| speech_start < end)
}

fn malformed_text_gaps(speech: &[(u64, u64)], sorted: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let mut gaps = Vec::new();
    for &(start, end) in speech {
        let mut cursor = start;
        for &(text_start, text_end) in sorted {
            if text_end <= cursor {
                continue;
            }
            if text_start >= end {
                break;
            }
            let missing_end = text_start.min(end);
            if missing_end.saturating_sub(cursor) >= MIN_UNCOVERED_MS {
                gaps.push((cursor, missing_end));
            }
            cursor = cursor.max(text_end).min(end);
        }
        if end.saturating_sub(cursor) >= MIN_UNCOVERED_MS {
            gaps.push((cursor, end));
        }
    }
    gaps
}
