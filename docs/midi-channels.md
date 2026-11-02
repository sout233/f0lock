# MIDI By Channel

Select **Audio Settings → Pitch Source: MIDI → MIDI Mode: By Channel**.
Choose **Ch 1–16** beside the selected Point's EQ type. Points default to
channels 1–8, and multiple Points can follow the same channel.

Each channel uses last-note priority. Releasing its latest note returns to
the previous held or sustained note on that channel. Pitch bend, sustain,
note expression, and channel reset messages remain channel-specific.
Notes on other channels do not steal a Point's pitch assignment.

Channel assignments are saved as per-Point parameters, independently for
the Track EQ and Sidechain EQ banks. Point Ratio still applies to the note's
frequency. The channel selector is hidden outside MIDI By Channel mode.

These are MIDI channels: all Points continue to process the plugin's audio
input. Lookahead delays the per-Point pitch decisions together with the audio.
