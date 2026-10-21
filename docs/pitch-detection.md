# Pitch detection

Select **Pitch Detection** in the header menu. The selection is a global,
automatable enum parameter (`pitch_detection`), saved by the host with the
project. Stable state IDs are `mpm`, `mpm_tracking`, and `pyin`. The default is
MPM, retaining the behavior of projects saved before this parameter existed.

## Modes

**MPM** retains the original McLeod detector and its relative-peak selection
constant of 0.93. Gate and Clarity retain their existing meanings.

**MPM + Tracking** examines all refined positive NSDF peaks, rather than
discarding competing periods immediately. Candidate observations combine
periodicity with harmonic support from a Hann-windowed FFT of the detector
signal. Harmonics are weighted by inverse harmonic number. A causal Viterbi
decoder favors a continuous pitch trajectory. An ambiguous octave candidate
must overcome the existing trajectory's accumulated evidence. It does not
hold a frequency that has no supporting candidate in the current frame.

**pYIN** computes YIN's cumulative mean normalized difference function,
integrates a Beta(2, 18) prior over 100 threshold intervals, and retains the
resulting pitch probabilities. Unsuccessful thresholds contribute 1% of
their probability to the absolute-minimum candidate; the remainder represents
unvoiced audio. Period refinement uses the unnormalized difference function
to reduce the short-period interpolation bias near the upper frequency limit.
This is probabilistic YIN, not ordinary YIN followed by a smoother.

## Streaming model and timing

Both enhanced modes use 10-cent pitch bins over 50–1000 Hz, with one voiced
and one unvoiced state per pitch bin. Both classes share the same pitch
transition kernel. Local transitions have triangular weights, scaled from
the paper's 2.5-semitone window per 256/44100-second analysis hop. Voicing changes
have a 1% transition prior. A separate 1% uniform new-note component allows
genuine large pitch changes, including octave jumps, without a silent gap.
These are model priors, not hard octave locks or maximum-jump restrictions.

The pYIN implementation adapts the paper's offline HMM to a streaming decoder:
it retains the paired voicing states and commits decisions after three analysis hops.
MPM + Tracking commits the current endpoint with no additional lookahead.
There is no offline whole-recording pass. All buffers and transition weights
are allocated at initialization; processing and algorithm changes allocate
nothing on the audio thread.

All modes use the existing 1 kHz lowpass, decimation, 1024-sample analysis
window, and 128-sample hop. At a 48 kHz host rate, analysis runs at 12 kHz:
the window spans 85.33 ms, each hop is 10.67 ms, and pYIN's additional lookahead
is 32 ms. At 44.1 kHz, the additional lookahead is about 26.12 ms. These times
describe detection response, not an audio delay. The main audio path and DAW
reported latency remain zero samples. The Tracking knob independently smooths
the EQ center frequency after detection.

## Attack and Release

Attack and Release are global, automatable parameters saved with the project.
Attack defaults to 10 ms (range 0.1–300 ms); Release defaults to 100 ms
(range 1–2000 ms). They control each enabled point's processing amount,
independently of the Tracking frequency smoother and overall Mix control.

The processing amount ramps linearly from dry to fully filtered over Attack,
and from fully filtered to dry over Release. This definition also applies to
High-pass, Low-pass, and Notch filters. Intermediate amounts mix the original
and filtered signals, so the response is not necessarily a linear ramp in dB.
Filter Gain is not faded separately. Partial ramps reverse from their current
amount at the newly selected slope, without resetting the filter states.

When detection becomes invalid, the frequency smoother freezes at its last
position while Release fades the processing out. A new valid estimate resumes
frequency tracking and Attack from the current processing amount. The filter
states clear only when the point becomes fully dry. Attack and Release begin
when the detector reports its decision; they add to detection response time
and do not add delay to the audio path.

Changing modes, resetting the plugin, or passing below Gate clears trajectory
history. Clarity is still a periodicity threshold for the selected candidate,
not a calibrated probability that the musical note is correct.

## Limits and verification

Temporal tracking cannot prove an ambiguous signal's intended musical
fundamental. Missing fundamentals, polyphony, or genuinely periodic subharmonics
can still produce octave errors. The original reported recording has not been
supplied, so the behavior on that recording remains unverified.

Tests cover competing YIN candidates, transient ±1/±2-octave alternatives,
sustained true octave changes, harmonics, antiphase stereo, silence and new
onsets, algorithm switching, sample rates from 8 to 192 kHz, frequency limits,
host gestures, CLAP state restoration, and audio-thread allocation checks.

Reference: Mauch and Dixon, [pYIN: A Fundamental Frequency Estimator Using
Probabilistic Threshold Distributions](https://webspace.eecs.qmul.ac.uk/s.e.dixon/pub/2014/MauchDixon-PYIN-ICASSP2014.pdf),
ICASSP 2014. The implementation is written for this project using the vendored
pitch-estimate transforms; no upstream pYIN implementation was copied.

## Sustained-note dropout fix

The first streaming version incorrectly collapsed all unvoiced pitch states
into one state with a strong self-loop. Voiced paths still paid the probability
cost of spreading transitions over pitch bins, while the collapsed unvoiced
path did not. A moderately noisy sustained note could therefore be recognized
at its clean attack and then lose all EQ processing despite high periodicity.

The decoder now keeps paired voiced/unvoiced states for every pitch bin,
applying the same pitch transition kernel to both. This corrects the model
without changing Gate, Clarity, or adding a hold timer. The regression command
is `cargo test --lib pitch::sustain_tests -- --nocapture`.
A deterministic 220 Hz tone with noise is compared across all three modes;
the test checks continuous voicing, band engagement, and measured audio gain.
