# F0LOCK Ψ

F0LOCK Ψ is the default detector for new Track EQ and Sidechain EQ instances.
Select it under **Pitch Detection** in the header menu. Use **Target → Range /
Reference** to guide tracking toward the intended voice in overlapping vocals.
Existing projects retain their saved selection; the state ID remains
`spectral_melody`.

## How it works

The detector scores multiple harmonic candidates and selects a continuous pitch
path with Viterbi decoding, rather than requiring the entire mixture to be
periodic. It uses a wider analysis band than MPM / pYIN:

- Eighth-order low-pass at `min(4000 Hz, analysis_rate × 0.3)` before decimation.
- 1024-sample Hann window, 128-sample hop and a zero-padded 4096-point FFT.
- 50–2000 Hz candidates at 10-cent intervals, using up to 16 harmonics.
- Range / Reference guidance and three hops of lookahead.

Buffers are preallocated; analysis and mode switching do not allocate on the
audio thread. Peak refinement is constrained to the search endpoints to retain
notes at exactly 2 kHz. Range / Reference also support 2 kHz; the original
detectors retain their existing 1 kHz search range, filter and tracking priors.

## Limits and response

This is pitch tracking, not source separation. Auto cannot always distinguish
the intended lead from octave harmonies or overlapping partials. Clarity is a
spectral evidence score, not calibrated accuracy or MPM periodicity.

At 48 kHz, the analysis window spans about 85 ms and lookahead adds 32 ms.
Tracking and Attack also affect response. Optional [Lookahead](lookahead.md)
adds audio alignment delay; it is off by default. Gate, Clarity, Tracking and
Attack / Release remain active.

## Validation

Run `cargo test --lib --locked` for harmony, target selection, noise, sustained
EQ, sample-rate and allocation checks.

For external audio, set `F0LOCK_MELODY_FIXTURE` to an interleaved stereo,
44.1 kHz, little-endian f32 file, then run:

```powershell
cargo test --lib --locked analyze_external_harmony_fixture -- --ignored --nocapture
```

The test writes per-frame CSV beside the fixture. On the supplied 20.4-second
vocal sample, output coverage was 78.9%, versus 7.0–15.6% for the other modes
with the same 250–600 Hz range and thresholds. Coverage is not pitch accuracy;
the sample has no annotated lead-pitch ground truth.

The external high-synth regression uses `F0LOCK_HIGH_SYNTH_FIXTURE` (the same
PCM format) and the `external_high_synth` test filter to check octave errors in Auto.
