//! `detect` performs no heap allocation in steady state. A counting allocator
//! wraps the system allocator and the test reads its counter around each call.
//!
//! Run this binary on its own with `--test-threads=1`. The counter is
//! process-wide, so a second test thread would add its own allocations to
//! the count. CI runs `cargo test --test zero_alloc -- --test-threads=1`.

mod common;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::Lcg;
use pitch_estimate::{Float, McLeodDetector, PitchDetector, YinDetector};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

/// Counts every allocation request and forwards it to the system allocator.
/// Implementing `GlobalAlloc` is unsafe by definition. This is test code only.
/// The library itself is `#![forbid(unsafe_code)]`.
struct Counting;

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        System.alloc(layout)
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        System.alloc_zeroed(layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        ALLOCATIONS.fetch_add(1, Ordering::SeqCst);
        System.realloc(ptr, layout, new_size)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn frames<T: Float>(len: usize, overflow_amp: f64, seed: u64) -> Vec<(&'static str, Vec<T>)> {
    let mut lcg = Lcg::new(seed);
    let sine = (0..len)
        .map(|i| {
            let t = i as f64 / 44_100.0;
            T::from_f64_lossy((2.0 * std::f64::consts::PI * 220.0 * t).sin())
        })
        .collect();
    let noise = (0..len)
        .map(|_| T::from_f64_lossy(lcg.next_unit()))
        .collect();
    let zero = vec![T::ZERO; len];
    let overflow = (0..len)
        .map(|i| {
            T::from_f64_lossy(if i % 2 == 0 {
                overflow_amp
            } else {
                -overflow_amp
            })
        })
        .collect();
    vec![
        ("sine", sine),
        ("noise", noise),
        ("all zero", zero),
        ("overflow amplitude", overflow),
    ]
}

/// Warm up once per frame type, then count allocations across repeated calls.
fn measure<T: Float, D: PitchDetector<T>>(
    detector: &mut D,
    frames: &[(&'static str, Vec<T>)],
    label: &str,
    failures: &mut Vec<String>,
) {
    for (_, frame) in frames {
        let _ = detector.detect(frame, 44_100);
    }
    for (name, frame) in frames {
        let before = ALLOCATIONS.load(Ordering::SeqCst);
        for _ in 0..3 {
            let _ = detector.detect(frame, 44_100);
            let _ = detector.detect(frame, 48_000);
        }
        let after = ALLOCATIONS.load(Ordering::SeqCst);
        if after != before {
            failures.push(format!("{label} {name}: {} allocations", after - before));
        }
    }
}

#[test]
fn detect_is_zero_alloc() {
    // The counter must see ordinary allocations, or a passing result would
    // mean nothing.
    let before = ALLOCATIONS.load(Ordering::SeqCst);
    let probe: Vec<u8> = vec![1; 1 << 12];
    assert!(
        ALLOCATIONS.load(Ordering::SeqCst) > before,
        "counter is live"
    );
    drop(probe);

    let mut failures = Vec::new();
    for &(frame_len, sub) in &[(2048usize, 1024usize), (1000, 500)] {
        let frames64: Vec<(&str, Vec<f64>)> = frames(frame_len, 1e152, 1_000_064);
        let frames32: Vec<(&str, Vec<f32>)> = frames(frame_len, 1e17, 1_000_032);

        let mut mpm = McLeodDetector::<f64>::new(frame_len, sub).unwrap();
        measure(
            &mut mpm,
            &frames64,
            &format!("mpm/f64 {frame_len}"),
            &mut failures,
        );
        let mut yin = YinDetector::<f64>::new(frame_len, sub).unwrap();
        measure(
            &mut yin,
            &frames64,
            &format!("yin/f64 {frame_len}"),
            &mut failures,
        );

        let mut mpm = McLeodDetector::<f32>::new(frame_len, sub).unwrap();
        measure(
            &mut mpm,
            &frames32,
            &format!("mpm/f32 {frame_len}"),
            &mut failures,
        );
        let mut yin = YinDetector::<f32>::new(frame_len, sub).unwrap();
        measure(
            &mut yin,
            &frames32,
            &format!("yin/f32 {frame_len}"),
            &mut failures,
        );
    }
    assert!(
        failures.is_empty(),
        "detect allocated:\n{}",
        failures.join("\n")
    );
}
