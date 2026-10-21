use super::{BINS, MAX_HZ, MIN_HZ, Target, TargetMode};
use pitch_estimate::McLeodDetector;
use realfft::{RealFftPlanner, RealToComplex, num_complex::Complex32};
use std::sync::Arc;

pub(super) struct Observations {
    pub probability: [f32; BINS],
    pub frequency: [f32; BINS],
    pub clarity: [f32; BINS],
}

impl Default for Observations {
    fn default() -> Self {
        Self {
            probability: [0.0; BINS],
            frequency: [0.0; BINS],
            clarity: [0.0; BINS],
        }
    }
}

impl Observations {
    pub fn clear(&mut self) {
        self.probability.fill(0.0);
        self.frequency.fill(0.0);
        self.clarity.fill(0.0);
    }

    pub fn add(&mut self, frequency: f32, clarity: f32, probability: f32) {
        if !(MIN_HZ..=MAX_HZ).contains(&frequency) {
            return;
        }

        let bin = (120.0 * (frequency / MIN_HZ).log2()).round() as usize;
        let total = self.probability[bin] + probability;

        if total > 0.0 {
            self.frequency[bin] =
                (self.frequency[bin] * self.probability[bin] + frequency * probability) / total;
            self.clarity[bin] =
                (self.clarity[bin] * self.probability[bin] + clarity * probability) / total;
        }

        self.probability[bin] = total;
    }
}

pub(super) struct SpectralEvidence {
    fft: Arc<dyn RealToComplex<f32>>,
    input: Vec<f32>,
    output: Vec<Complex32>,
    scratch: Vec<Complex32>,
    window: Vec<f32>,
    maximum: f32,
}

impl SpectralEvidence {
    pub fn new(size: usize) -> Self {
        let fft = RealFftPlanner::new().plan_fft_forward(size);

        Self {
            input: fft.make_input_vec(),
            output: fft.make_output_vec(),
            scratch: fft.make_scratch_vec(),
            fft,
            window: (0..size)
                .map(|i| 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / (size - 1) as f32).cos())
                .collect(),
            maximum: 0.0,
        }
    }

    pub fn analyze(&mut self, frame: &[f32]) {
        for ((input, sample), window) in self.input.iter_mut().zip(frame).zip(&self.window) {
            *input = sample * window;
        }

        self.fft
            .process_with_scratch(&mut self.input, &mut self.output, &mut self.scratch)
            .expect("Pitch evidence FFT failed");
        self.maximum = self.output.iter().map(|x| x.norm()).fold(0.0, f32::max);
    }

    fn support(&self, hz: f32, rate: f32) -> f32 {
        if self.maximum == 0.0 {
            return 0.0;
        }

        let mut support = 0.0;
        let harmonics = (MAX_HZ / hz).floor() as usize;

        for harmonic in 1..=harmonics {
            let bin = (hz * harmonic as f32 * self.input.len() as f32 / rate).round() as usize;
            let start = bin.saturating_sub(1);
            let end = (bin + 1).min(self.output.len() - 1);
            let amplitude = self.output[start..=end]
                .iter()
                .map(|x| x.norm())
                .fold(0.0, f32::max);
            support += amplitude / self.maximum / harmonic as f32;
        }

        support
    }
}

pub(super) fn mpm_observations(
    mpm: &mut McLeodDetector<f32>,
    frame: &[f32],
    rate: f32,
    evidence: &SpectralEvidence,
    out: &mut Observations,
    target: Target,
) {
    let mut voiced = 0.0_f32;

    mpm.candidates(frame, rate.round() as u32, |pitch| {
        let hz = pitch.frequency * rate / rate.round();

        if (MIN_HZ..=MAX_HZ).contains(&hz) && target.allows(hz) {
            voiced = voiced.max(pitch.clarity);
            let weight = (24.0 * (pitch.clarity - 1.0)).exp() * evidence.support(hz, rate);
            out.add(hz, pitch.clarity, weight);
        }
    });

    let total: f32 = out.probability.iter().sum();

    if total > 0.0 {
        for probability in &mut out.probability {
            *probability *= voiced / total;
        }
    }
}

pub(super) fn pyin_observations(
    curve: &[f32],
    difference: &[f32],
    rate: f32,
    out: &mut Observations,
    target: Target,
) {
    // Beta(2, 18): CDF(x) = 1 - (1-x)^18 * (1+18x).
    // Integrate each of the paper's 100 threshold intervals exactly.
    let cdf = |x: f64| 1.0 - (1.0 - x).powi(18) * (1.0 + 18.0 * x);
    let mut minimum = None;

    for i in 2..curve.len() - 1 {
        if curve[i] < curve[i - 1]
            && curve[i] <= curve[i + 1]
            && (target.mode == TargetMode::Auto || target.allows(rate / refine(difference, i).0))
            && minimum.is_none_or(|previous| curve[i] < curve[previous])
        {
            minimum = Some(i);
        }
    }

    let Some(minimum) = minimum else { return };

    for step in 1..=100 {
        let threshold = step as f32 / 100.0;
        let mut selected = minimum;
        let mut found = false;

        for i in 2..curve.len() - 1 {
            if curve[i] < threshold
                && curve[i] < curve[i - 1]
                && curve[i] <= curve[i + 1]
                && (target.mode == TargetMode::Auto
                    || target.allows(rate / refine(difference, i).0))
            {
                selected = i;
                found = true;
                break;
            }
        }

        // The original pYIN prior assigns 1% of unsuccessful thresholds to
        // the absolute minimum; the remaining mass represents unvoiced audio.
        let mass = (cdf(step as f64 / 100.0) - cdf((step - 1) as f64 / 100.0)) as f32
            * if found { 1.0 } else { 0.01 };
        // Refine pitch on the unnormalized difference: CMNDF's cumulative
        // denominator biases a parabola when periods contain only ~8 samples.
        let (lag, _) = refine(difference, selected);
        let (_, depth) = refine(curve, selected);
        out.add(rate / lag, (1.0 - depth).clamp(0.0, 1.0), mass);
    }
}

fn refine(curve: &[f32], i: usize) -> (f32, f32) {
    let difference = curve[i - 1] - curve[i + 1];
    let denominator = 2.0 * (curve[i - 1] - 2.0 * curve[i] + curve[i + 1]);
    let offset = if denominator == 0.0 {
        0.0
    } else {
        (difference / denominator).clamp(-0.5, 0.5)
    };
    (i as f32 + offset, curve[i] - difference * offset * 0.25)
}
