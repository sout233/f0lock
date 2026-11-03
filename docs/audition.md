# Listen

**Listen** sits to the left of **Active** in the processing strip and auditions
the selected point.

The whole chain keeps running. Only the output is narrowed: a band-pass centred
on the point's current frequency, 50 Hz either side, so the region is heard as
the EQ actually leaves it. Nothing is muted and nothing comes from the dry
signal, which also means neighbouring points that overlap the window are part of
what you hear.

The window width is set on the strip next to **Active**: click the control to
start or stop listening, drag it sideways to widen or narrow the window between
10 and 500 Hz either side. The width is shown on the control and stays put
between points, so it can be changed without switching the audition on. It is
measured in hertz rather than tied to the point's Q or gain, so a low point is
heard as wide as a high one.

The window follows the point's pitch, so in Track EQ it rides the tracked pitch
times that point's F0 Ratio. It is also drawn on the spectrum while listening,
and the other points dim so the one being auditioned stands out.

Mix and Output still apply to what is monitored. Global Bypass still returns dry
audio.

The audition follows the selection: choosing another point moves it there,
closing a point hands it to whichever point becomes selected next, and it stops
when no point is left. It is per editor and transient — not a parameter, so it
is not automated, not saved with the project, and does not change the host's
parameter list. Closing the plugin window stops it.

It uses the point selected in the bank being shown, so Track and Sidechain each
audition their own points.
