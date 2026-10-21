use super::{BINS, MIN_HZ, Target, candidates::Observations};
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;

const HARMONICS: usize = 16;

#[derive(Clone, Copy)]
struct Peak {
    hz: f32,
    strength: f32,
    evidence: f32,
}

/// Harmonic salience keeps simultaneous pitch hypotheses instead of requiring
/// the complete mixed waveform to have a single repeating period.
pub(super) struct MelodyEvidence {
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
    window: Vec<f32>,
    magnitude: Vec<f32>,
    prefix: Vec<f32>,
    peaks: Vec<Peak>,
    grid: [f32; BINS],
    weights: [f32; HARMONICS],
    salience: [f32; BINS],
    frequency: [f32; BINS],
    clarity: [f32; BINS],
}

impl MelodyEvidence {
    pub fn new(size: usize) -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(size * 4);
        let bins = size * 2 + 1;

        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos())
                .collect(),
            magnitude: vec![0.0; bins],
            prefix: vec![0.0; bins + 1],
            // At most one local maximum per two FFT bins.
            peaks: Vec::with_capacity(bins.div_ceil(2)),
            grid: std::array::from_fn(|i| MIN_HZ * 2.0_f32.powf(i as f32 / 120.0)),
            weights: std::array::from_fn(|i| 1.0 / ((i + 1) as f32).sqrt()),
            salience: [0.0; BINS],
            frequency: [0.0; BINS],
            clarity: [0.0; BINS],
        }
    }

    pub fn analyze(&mut self, frame: &[f32], rate: f32, target: Target, out: &mut Observations) {
        self.input.fill(0.0);

        for ((input, sample), window) in self.input.iter_mut().zip(frame).zip(&self.window) {
            *input = sample * window;
        }

        self.fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .expect("F0LOCK Ψ FFT failed");

        let bin_hz = rate / self.input.len() as f32;
        let end = (rate * 0.3).min(4000.0) / bin_hz;
        let end = end.floor() as usize;
        let mut maximum = 0.0_f32;
        self.prefix[0] = 0.0;

        for (i, value) in self.output.iter().enumerate() {
            let magnitude = value.norm();
            self.magnitude[i] = magnitude;
            self.prefix[i + 1] = self.prefix[i] + magnitude;
            maximum = maximum.max(magnitude);
        }

        self.peaks.clear();

        if maximum == 0.0 {
            return;
        }

        for i in 2..end {
            let center = self.magnitude[i];

            if center <= self.magnitude[i - 1] || center <= self.magnitude[i + 1] {
                continue;
            }

            // Normalized amplitude interpolation also handles zero neighbours;
            // taking their logarithm would make the peak frequency NaN.
            let left = self.magnitude[i - 1] / center;
            let right = self.magnitude[i + 1] / center;
            let offset = 0.5 * (left - right) / (left - 2.0 + right);
            let a = i.saturating_sub(40);
            let b = i.saturating_sub(8);
            let c = (i + 9).min(end);
            let d = (i + 41).min(end);
            let background = (self.prefix[b] - self.prefix[a] + self.prefix[d] - self.prefix[c])
                / (b - a + d - c) as f32;
            // Relative spectral floor prevents whitening empty bins into peaks.
            let ratio = center / (background + maximum * 0.01);

            self.peaks.push(Peak {
                hz: (i as f32 + offset) * bin_hz,
                strength: (ratio - 2.0).max(0.0).sqrt(),
                evidence: (std::f32::consts::FRAC_PI_4 * ratio * ratio
                    - ((BINS * HARMONICS) as f32).ln())
                .max(0.0),
            });
        }

        self.salience.fill(0.0);
        self.clarity.fill(0.0);

        if self.peaks.is_empty() {
            return;
        }

        for (i, hz) in self.grid.iter().copied().enumerate() {
            let mut weighted_hz = 0.0;
            let mut evidence = 0.0;

            for (h, weight) in self.weights.iter().copied().enumerate() {
                let harmonic = (h + 1) as f32;
                let expected = hz * harmonic;

                if expected >= end as f32 * bin_hz {
                    break;
                }

                let next = self.peaks.partition_point(|peak| peak.hz < expected);
                let left = next.saturating_sub(1);
                let right = next.min(self.peaks.len() - 1);
                let index = if (self.peaks[left].hz - expected).abs()
                    < (self.peaks[right].hz - expected).abs()
                {
                    left
                } else {
                    right
                };
                let peak = self.peaks[index];
                let sigma = (rate / frame.len() as f32 * 0.2).max(expected * 0.005);
                let distance = (peak.hz - expected) / sigma;
                let agreement = (-0.5 * distance * distance).exp();
                let contribution = peak.strength * agreement * weight;
                self.salience[i] += contribution;
                weighted_hz += contribution * peak.hz / harmonic;
                evidence += peak.evidence * agreement * agreement;
            }

            if self.salience[i] > 0.0 {
                self.frequency[i] = weighted_hz / self.salience[i];
                self.clarity[i] = 1.0 - (-evidence).exp();
            }
        }

        let mut voiced = 0.0_f32;

        for i in 0..BINS {
            let score = self.salience[i];

            if score == 0.0
                || (i > 0 && score <= self.salience[i - 1])
                || (i + 1 < BINS && score <= self.salience[i + 1])
                || !target.allows(self.frequency[i])
            {
                continue;
            }

            // Sharpen salience while retaining competing voices for Viterbi.
            let clarity = self.clarity[i];
            out.add(self.frequency[i], clarity, score.powi(4) * clarity);
            voiced = voiced.max(clarity);
        }

        let total: f32 = out.probability.iter().sum();

        if total > 0.0 {
            for probability in &mut out.probability {
                *probability = (*probability / total) * voiced;
            }
        }
    }
}
