# Q Follow

Each point has a bipolar, automatable **Q Follow** control (−2 to +2).
The Q knob sets the base Q at a source pitch of 220 Hz:

`effective Q = base Q × (source pitch / 220 Hz)^Follow`

- **0:** fixed Q, preserving the previous behavior and existing projects.
- **+1:** Q doubles per octave; relative bandwidth narrows while approximate
  bandwidth in Hz stays constant.
- **−1:** Q halves per octave, widening the band as pitch rises.
- **+2:** Q quadruples per octave, also narrowing approximate bandwidth in Hz.

Effective Q uses the existing 0.3–16 range and parameter smoothing. It holds
during pitch loss rather than following a placeholder frequency. The source
pitch is taken before the point's F0 Ratio; MIDI per-point modes use that point's
note. The spectrum response uses the actual smoothed filter coefficients.

Track EQ and Sidechain EQ store Q Follow independently for all eight points.
The control shares Q's availability: a 6 dB/oct cut has no Q control. Right-click
accepts an exact number, and double-click resets Q Follow to zero.
