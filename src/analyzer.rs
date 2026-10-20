use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;

pub const BINS: usize = 256;
pub const MIN_HZ: f32 = 20.0;

pub fn max_frequency(sample_rate: f32) -> f32 {
    20000.0_f32.min(sample_rate * 0.49)
}

pub fn bin_frequency(index: usize, max_hz: f32) -> f32 {
    MIN_HZ * (max_hz / MIN_HZ).powf(index as f32 / (BINS - 1) as f32)
}

#[derive(Clone, Copy)]
struct BinMapping {
    first: usize,
    last: usize,
    k0: usize,
    k1: usize,
    weight: f32,
    blend: f32,
}

pub struct Analyzer {
    rings: [Vec<f32>; 2],
    window: Vec<f32>,
    position: usize,
    filled: usize,
    hop: usize,
    hop_size: usize,
    forward: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    spectrum: Vec<Complex32>,
    scratch: Vec<Complex32>,
    peaks: Vec<f32>,
    mapping: [BinMapping; BINS],
    normalization: f32,
    pub display: [f32; BINS],
}

impl Analyzer {
    pub fn new(sample_rate: f32) -> Self {
        let size = ((sample_rate * 0.085).ceil() as usize).next_power_of_two();
        let mut planner = RealFftPlanner::new();
        let forward = planner.plan_fft_forward(size);
        let window: Vec<_> = (0..size)
            .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos())
            .collect();
        let normalization = 2.0 / window.iter().sum::<f32>();
        let max_hz = max_frequency(sample_rate);
        let max_bin = size / 2;

        let mapping = std::array::from_fn(|i| {
            let ratio = (max_hz / MIN_HZ).powf(0.5 / (BINS - 1) as f32);
            let center = bin_frequency(i, max_hz);
            let lower = if i == 0 { MIN_HZ } else { center / ratio };
            let upper = if i == BINS - 1 {
                max_hz
            } else {
                center * ratio
            };
            let k_c = (center * size as f32 / sample_rate).min(max_bin as f32);
            let k_l = (lower * size as f32 / sample_rate)
                .max(0.0)
                .min(max_bin as f32);
            let k_u = (upper * size as f32 / sample_rate)
                .max(0.0)
                .min(max_bin as f32);
            let span = k_u - k_l;
            let first = (k_l.floor() as usize).min(max_bin);
            let last = (k_u.ceil() as usize).min(max_bin).max(first);

            let k0 = (k_c.floor() as usize).min(max_bin);
            let k1 = (k0 + 1).min(max_bin);
            let t = (k_c - k0 as f32).clamp(0.0, 1.0);
            let weight = t * t * (3.0 - 2.0 * t);
            let blend = (span - 1.0).clamp(0.0, 1.0);

            BinMapping {
                first,
                last,
                k0,
                k1,
                weight,
                blend,
            }
        });

        Self {
            rings: std::array::from_fn(|_| vec![0.0; size]),
            window,
            position: 0,
            filled: 0,
            hop: 0,
            hop_size: (sample_rate / 30.0).round() as usize,
            input: forward.make_input_vec(),
            spectrum: forward.make_output_vec(),
            scratch: forward.make_scratch_vec(),
            peaks: vec![0.0; size / 2 + 1],
            forward,
            mapping,
            normalization,
            display: [-100.0; BINS],
        }
    }

    pub fn reset(&mut self) {
        for ring in &mut self.rings {
            ring.fill(0.0);
        }

        self.position = 0;
        self.filled = 0;
        self.hop = 0;
        self.display.fill(-100.0);
    }

    pub fn push(&mut self, input: [f32; 2], active: bool) {
        if !active {
            if self.filled != 0 {
                self.reset();
            }
            return;
        }

        let size = self.window.len();

        for (ring, sample) in self.rings.iter_mut().zip(input) {
            ring[self.position] = sample;
        }

        self.position = (self.position + 1) % size;
        self.filled = (self.filled + 1).min(size);
        self.hop += 1;

        if self.filled == size && self.hop >= self.hop_size {
            self.hop = 0;
            self.analyze();
        }
    }

    fn analyze(&mut self) {
        self.peaks.fill(0.0);
        let size = self.window.len();

        for ring in &self.rings {
            let mean = ring.iter().sum::<f32>() / size as f32;

            for (i, sample) in self.input.iter_mut().enumerate() {
                *sample = (ring[(self.position + i) % size] - mean) * self.window[i];
            }

            self.forward
                .process_with_scratch(&mut self.input, &mut self.spectrum, &mut self.scratch)
                .expect("Input spectrum FFT failed");

            // Preserve both channels' content, including antiphase material.
            for (peak, bin) in self.peaks.iter_mut().zip(&self.spectrum) {
                *peak = peak.max(bin.norm_sqr());
            }
        }

        for (display, map) in self.display.iter_mut().zip(&self.mapping) {
            let peak_power = self.peaks[map.first..=map.last]
                .iter()
                .copied()
                .fold(0.0_f32, f32::max);
            let peak_amp = peak_power.sqrt() * self.normalization;

            let a0 = self.peaks[map.k0].sqrt() * self.normalization;
            let a1 = self.peaks[map.k1].sqrt() * self.normalization;
            let interp_amp = a0 * (1.0 - map.weight) + a1 * map.weight;

            let amplitude = interp_amp * (1.0 - map.blend) + peak_amp * map.blend;
            *display = 20.0 * amplitude.max(1e-8).log10();
        }
    }
}
