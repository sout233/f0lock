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

While a point has Dynamics enabled, the spectrum shades the range between its
static curve and the curve at full range. The shading stays faint on purpose,
since several dynamic points can overlap in the same region, and the selected
point's is a step stronger than the rest.

The selected point also carries a two-arrow grip on the bottom edge of its
shading. Dragging the grip up or down sets Range without leaving the plot: it
follows the graph's own vertical scale, 48 dB over the plot height, and stops
at 0 dB and −24 dB. The grip keeps clear of the point marker so both stay
draggable when Range is near zero, and it is drawn in the point's colour,
greyed out when the point is disabled. Only one grip is on screen at a time,
because it and the Range knob always edit the same point.

The wheel sets the point's Q while the grip is held, the same as it does while
an EQ point is held; see [spectrum editing](spectrum-editing.md).

Each point's settings are automatable, saved with the project and independent
between Track EQ and Sidechain EQ. Global Attack / Release still control EQ
engagement when pitch is acquired or lost; they do not control compression.
