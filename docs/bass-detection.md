# Bass detection range

Select **Audio Settings → Detection Range → Bass: 20–300 Hz**.
All four detection algorithms support it. Standard and Low remain available;
the default range is unchanged. MIDI pitch tracking does not use this setting.

The Bass profile uses a separate, preallocated detector with an analysis rate
near 4 kHz, a 1024-sample window, and a 64-sample hop. Only the selected profile
runs. An eighth-order low-pass filter precedes downsampling; at 48 kHz the
cutoff is 1.2 kHz, retaining harmonics above the 300 Hz fundamental limit.
Temporal transition rates account for the longer hop duration.

At 48 kHz, the window spans 256 ms and a delayed decision spans up to about
304 ms. Lookahead reports the corresponding latency and raises shorter delay
selections to this minimum. With Lookahead off, audio is not delayed, but the
detector still needs time to establish a pitch.

Pitch candidates are limited to 20–300 Hz. Endpoint interpolation is constrained
within the existing ten-cent pitch-grid endpoint bins; evidence and Clarity
are unchanged. Target Range and Reference controls can reach 20 Hz, and their
effective intervals are intersected with the selected detection range. An
incompatible target interval produces no valid pitch and is marked in the UI.
Point Ratio still allows EQ frequencies above 300 Hz.

Tests cover both frequency endpoints, all backends, multiple sample rates,
manual targets, weak fundamentals with harmonics above 300 Hz, anti-alias rejection,
descending glides, Lookahead, sidechain processing, and allocation-free switching.
