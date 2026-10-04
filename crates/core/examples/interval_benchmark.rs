//! Synthetic interval microbenchmark; no media/provider/hardware accuracy claims.
//! Run: cargo run --release -p course-core --example interval_benchmark --offline
use course_core::quality_intervals::uncovered_speech;
use serde_json::json;
use std::{hint::black_box, time::Instant};

type Interval = (u64, u64);

fn original_scan(speech: &[Interval], text: &[Interval]) -> Vec<Interval> {
    let mut text = text.to_vec();
    text.sort_by_key(|&(start, _)| start);
    let mut gaps = Vec::new();
    for &(start, end) in speech {
        let mut cursor = start;
        for &(a, b) in &text {
            if b <= cursor {
                continue;
            }
            if a >= end {
                break;
            }
            let missing_end = a.min(end);
            if missing_end.saturating_sub(cursor) >= 1500 {
                gaps.push((cursor, missing_end));
            }
            cursor = cursor.max(b).min(end);
        }
        if end.saturating_sub(cursor) >= 1500 {
            gaps.push((cursor, end));
        }
    }
    gaps
}

fn timeline(count: u64) -> (Vec<Interval>, Vec<Interval>) {
    let mut speech = Vec::new();
    let mut text = Vec::new();
    for i in 0..count {
        let start = i * 6000;
        speech.push((start, start + 5000));
        match i % 4 {
            0 => text.push((start + 1500, start + 3500)),
            1 => text.push((start, start + 5000)),
            2 => (),
            _ => {
                text.push((start + 500, start + 2200));
                text.push((start + 1800, start + 3501));
                text.push((start + 2400, start + 2400));
            }
        }
    }
    // Sorting is included in both measured implementations.
    text.reverse();
    (speech, text)
}

fn measure(
    algorithm: fn(&[Interval], &[Interval]) -> Vec<Interval>,
    speech: &[Interval],
    text: &[Interval],
) -> (Vec<Interval>, f64) {
    let start = Instant::now();
    let result = black_box(algorithm(black_box(speech), black_box(text)));
    (result, start.elapsed().as_secs_f64() * 1000.0)
}

fn median(values: &[f64]) -> f64 {
    let mut values = values.to_vec();
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

fn main() {
    let mut cases = Vec::new();
    for count in [1000, 10000, 30000] {
        let (speech, text) = timeline(count);
        let expected = original_scan(&speech, &text);
        assert_eq!(uncovered_speech(&speech, &text), expected);
        let mut old_samples = Vec::new();
        let mut new_samples = Vec::new();
        for trial in 0..5 {
            // Alternate execution order to reduce systematic warm-cache bias.
            let (old, new) = if trial % 2 == 0 {
                (
                    measure(original_scan, &speech, &text),
                    measure(uncovered_speech, &speech, &text),
                )
            } else {
                let new = measure(uncovered_speech, &speech, &text);
                (measure(original_scan, &speech, &text), new)
            };
            assert_eq!(old.0, expected);
            assert_eq!(new.0, expected);
            old_samples.push(old.1);
            new_samples.push(new.1);
        }
        let old_ms = median(&old_samples);
        let new_ms = median(&new_samples);
        cases.push(json!({
            "speech_intervals": speech.len(), "text_intervals": text.len(),
            "timeline_hours": count as f64 * 6000.0 / 3600000.0,
            "uncovered_intervals": expected.len(), "equal": true,
            "old_samples_ms": old_samples, "new_samples_ms": new_samples,
            "old_median_ms": old_ms, "new_median_ms": new_ms,
            "speedup": old_ms / new_ms,
        }));
    }
    println!("{}", serde_json::to_string_pretty(&json!({
        "benchmark": "quality_interval_difference", "synthetic": true,
        "profile": if cfg!(debug_assertions) { "debug" } else { "release" },
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "threshold_ms": 1500, "trials_per_case": 5,
        "peak_process_working_set_bytes": peak_working_set(),
        "memory_scope": "whole benchmark process, including both algorithms and generated fixtures; not a per-algorithm allocation comparison",
        "timing_includes": "text sorting, merging (new), interval difference and output allocation",
        "timing_excludes": "JSON parsing, desktop issue construction, diagnostics and fingerprinting",
        "cases": cases,
    })).unwrap());
}

#[cfg(windows)]
fn peak_working_set() -> Option<usize> {
    #[repr(C)]
    struct Counters {
        size: u32,
        page_faults: u32,
        values: [usize; 8],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
    }
    #[link(name = "psapi")]
    unsafe extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut std::ffi::c_void,
            counters: *mut Counters,
            size: u32,
        ) -> i32;
    }
    let mut counters = Counters {
        size: std::mem::size_of::<Counters>() as u32,
        page_faults: 0,
        values: [0; 8],
    };
    // SAFETY: documented Windows PROCESS_MEMORY_COUNTERS layout and valid output buffer.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<Counters>() as u32,
        )
    };
    (ok != 0).then_some(counters.values[0])
}
#[cfg(not(windows))]
fn peak_working_set() -> Option<usize> {
    None
}
