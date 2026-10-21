# Dynamics

The selected point's EQ controls stay on the left, with Dynamics on the right.
All knobs remain visible. Click **Dynamics** to enable or disable dynamic
processing; the round indicator lights up when the selected point is enabled.
Dynamic parameters can be adjusted while processing is off.

| Control | Meaning |
| --- | --- |
| Threshold | Filtered detector RMS level that starts compression, in dBFS |
| Range | Maximum extra attenuation, from 0 to −24 dB |
| Dyn.Attack | Time constant for increasing attenuation |
| Dyn.Release | Time constant for recovering from attenuation |

Compression uses a fixed 4:1 ratio above Threshold, limited by Range. The
result is added to the point's static Gain. Dynamics defaults to off and supports
Peak, Low Shelf and High Shelf; other filter types retain static processing.

Track EQ detects the unprocessed main input; Sidechain EQ detects the external
sidechain. Peak uses a unity-center-gain bandpass at the point's F0 ratio and Q.
Shelves detect the corresponding low/high frequency region. Stereo channel
powers are averaged, so opposite-polarity audio does not cancel the detector.
RMS detection uses a 5 ms power smoother before Dyn.Attack / Dyn.Release.
The filter's existing coefficient smoothing also remains active.

With Lookahead enabled, detector samples and controls are delayed with the main
audio. Dynamics adds no further reported latency. It does not provide separate
transient lookahead. The response curve shows the actual filter response, and
the Dynamics section shows the extra gain adjustment beside its enable switch.

Each point's settings are automatable, saved with the project and independent
between Track EQ and Sidechain EQ. Global Attack / Release still control EQ
engagement when pitch is acquired or lost; they do not control compression.
