# Spectrum editing

Points are edited directly on the spectrum. Dragging a point sideways moves its
F0 Ratio, which is the point's frequency relative to the tracked pitch; dragging
it up or down sets Gain, for the types that have one. Both keep working while a
note is being tracked, so a point follows the pitch and can be placed by ear.

While a point is held, the mouse wheel sets its Q. One notch scales Q by ×1.15,
multiplicatively, so a notch covers the same musical distance at a narrow Q as
at a wide one; the parameter's own 0.3–16 limits still apply. The wheel only
works during a drag: hovering a point and scrolling changes nothing, so the plot
stays safe to scroll over. First-order cuts have no Q control, and they ignore
the wheel for the same reason their Q knob is disabled.

Holding a dynamic point's range grip works the same way, so Q can be set without
letting go of the grip. See [dynamics](dynamics.md) for the grip itself.
