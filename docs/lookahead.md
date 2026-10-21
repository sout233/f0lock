# Lookahead and first-pitch acquisition

When every EQ band is fully dry, the first valid pitch initializes the filter
frequency directly before Attack. This removes the sweep from the 150 Hz
placeholder. Tracking still smooths subsequent changes and reacquisition during
Release; no filter frequency is snapped while a band is still processing audio.

## Lookahead

Use **Lookahead** in the toolbar below the spectrum to buffer audio while
the detector analyzes the incoming signal. The setting is global, saved with the
project, off by default and not automatable. Choose Off, Minimum, 150, 200 or
300 ms. The button shows the active delay. It works in Track EQ and Sidechain EQ.

Minimum uses the common full-window and decoder budget: about 117.3 ms at
48 kHz, or 175.9 ms at 8 kHz. The default enabled preset is 150 ms; presets below
the required budget use that budget instead. Larger presets add waiting time,
not analysis resolution. Changing detectors does not change the chosen latency.
Audio Settings shows the actual sample count and duration, also reported to the
host for delay compensation. Mix, Dry and Bypass retain the same delay.

Pitch decisions are dated at the center of their original analysis window,
including the decoder's three-hop delay where applicable. On first acquisition,
the observed pitch can also prepare a fully dry EQ during that window's leading
half. Attack waits for the buffered detection source to reach the existing Gate
level, then remains engaged across waveform zero crossings. This onset check
does not replace the detector's RMS Gate or Clarity threshold.

EQ settings and automation travel with their audio samples. The spectrum and
meters follow the delayed main input. Buffers are preallocated; processing,
switching and reset do not allocate on the audio thread.

Change Lookahead while stopped: switching clears the buffers and may cause the
host to restart playback as its delay compensation changes. A reset discards
pending audio and pitch decisions. Lookahead cannot guarantee a correct first
pitch when the analysis itself is ambiguous or below the configured thresholds.

## Checks

`cargo test --lib --locked` checks initial frequency placement, onset EQ gain,
Attack timing, ongoing Tracking, source selection, exact stereo delay, delayed
automation, Bypass, reset and allocation behavior. The packaged CLAP test also
checks saved settings, reported latency, impulse timing, tail and runtime disable.
