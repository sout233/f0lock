use super::{BINS, LOOKAHEAD, PERIODIC_BINS, STATES, UNVOICED, candidates::Observations};

const HISTORY: usize = LOOKAHEAD + 1;
const VOICE_SWITCH: f32 = 0.01;
const NEW_NOTE: f32 = 0.01;

struct Frame {
    threshold: f32,
    previous: [u16; STATES],
    frequency: [f32; BINS],
    clarity: [f32; BINS],
}

pub(super) struct Tracker<const PITCH_BINS: usize = PERIODIC_BINS> {
    score: [f32; STATES],
    next: [f32; STATES],
    history: [Frame; HISTORY],
    pub(super) frames: usize,
    radius: usize,
    transitions: Vec<f32>,
}

impl<const PITCH_BINS: usize> Tracker<PITCH_BINS> {
    pub fn new(hop_seconds: f32) -> Self {
        // Scale the paper's 2.5-semitone triangular transition window from
        // its 256/44100-second hop to this detector's hop duration.
        let radius = (25.0 * hop_seconds / (256.0 / 44100.0)).ceil() as usize;
        let normalization: [f32; PITCH_BINS] = std::array::from_fn(|i| {
            (i.saturating_sub(radius)..=(i + radius).min(PITCH_BINS - 1))
                .map(|j| (radius + 1 - i.abs_diff(j)) as f32)
                .sum::<f32>()
        });
        let transitions = (0..PITCH_BINS)
            .flat_map(|i| {
                (0..=radius).map(move |distance| {
                    let local = (radius + 1 - distance) as f32 / normalization[i];
                    ((1.0 - NEW_NOTE) * local + NEW_NOTE / PITCH_BINS as f32).ln()
                })
            })
            .collect();

        let mut tracker = Self {
            score: [f32::NEG_INFINITY; STATES],
            next: [f32::NEG_INFINITY; STATES],
            history: std::array::from_fn(|_| Frame {
                threshold: 0.0,
                previous: [0; STATES],
                frequency: [0.0; BINS],
                clarity: [0.0; BINS],
            }),
            frames: 0,
            radius,
            transitions,
        };
        tracker.reset();
        tracker
    }

    pub fn reset(&mut self) {
        self.score.fill(f32::NEG_INFINITY);
        self.score[UNVOICED..UNVOICED + PITCH_BINS].fill(-(PITCH_BINS as f32).ln());
        self.frames = 0;
    }

    pub fn process(
        &mut self,
        observation: &Observations,
        threshold: f32,
        lag: usize,
    ) -> (f32, f32) {
        assert!(lag < HISTORY, "Pitch decoder lookahead exceeds its history");

        let best_voiced = (0..PITCH_BINS)
            .max_by(|&a, &b| self.score[a].total_cmp(&self.score[b]))
            .unwrap();
        let best_unvoiced = (UNVOICED..UNVOICED + PITCH_BINS)
            .max_by(|&a, &b| self.score[a].total_cmp(&self.score[b]))
            .unwrap();
        let frame = &mut self.history[self.frames % HISTORY];
        frame.frequency = observation.frequency;
        frame.clarity = observation.clarity;
        frame.threshold = threshold;
        self.next.fill(f32::NEG_INFINITY);

        let stay = (1.0 - VOICE_SWITCH).ln();
        let switch = VOICE_SWITCH.ln();
        let jump = (NEW_NOTE / PITCH_BINS as f32).ln();
        let unvoiced = (1.0 - observation.probability.iter().sum::<f32>()).clamp(0.0, 1.0);

        for i in 0..PITCH_BINS {
            let mut voiced = (best_voiced, self.score[best_voiced] + jump);
            let mut silent = (best_unvoiced, self.score[best_unvoiced] + jump);

            for j in i.saturating_sub(self.radius)..=(i + self.radius).min(PITCH_BINS - 1) {
                let transition = self.transitions[j * (self.radius + 1) + i.abs_diff(j)];
                let voiced_candidate = self.score[j] + transition;
                let silent_candidate = self.score[j + UNVOICED] + transition;

                if voiced_candidate > voiced.1 {
                    voiced = (j, voiced_candidate);
                }

                if silent_candidate > silent.1 {
                    silent = (j + UNVOICED, silent_candidate);
                }
            }

            // Both voicing classes use the same pitch transition kernel.
            // A single unvoiced state with a self-loop unfairly outcompetes
            // voiced paths, whose probability is spread over pitch bins.
            let (parent, score) = if voiced.1 + stay > silent.1 + switch {
                (voiced.0, voiced.1 + stay)
            } else {
                (silent.0, silent.1 + switch)
            };
            self.next[i] = score + observation.probability[i].ln();
            frame.previous[i] = parent as u16;

            let (parent, score) = if silent.1 + stay > voiced.1 + switch {
                (silent.0, silent.1 + stay)
            } else {
                (voiced.0, voiced.1 + switch)
            };
            self.next[i + UNVOICED] = score + unvoiced.ln();
            frame.previous[i + UNVOICED] = parent as u16;
        }

        let mut state = (0..STATES)
            .max_by(|&a, &b| self.next[a].total_cmp(&self.next[b]))
            .unwrap();
        let maximum = self.next[state];
        assert!(maximum.is_finite(), "Pitch HMM has no reachable state");

        for (score, next) in self.score.iter_mut().zip(self.next) {
            *score = next - maximum;
        }

        self.frames += 1;

        if self.frames <= lag {
            return (0.0, 0.0);
        }

        for offset in 0..lag {
            state = self.history[(self.frames - 1 - offset) % HISTORY].previous[state] as usize;
        }

        if state >= UNVOICED {
            return (0.0, 0.0);
        }

        let frame = &self.history[(self.frames - 1 - lag) % HISTORY];
        let clarity = frame.clarity[state];
        let frequency = if clarity >= frame.threshold {
            frame.frequency[state]
        } else {
            0.0
        };
        (frequency, clarity)
    }
}
