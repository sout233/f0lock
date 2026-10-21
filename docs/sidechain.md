# Operating modes

The header button selects **Track EQ** or **Sidechain EQ**. The default is
Track EQ, including when loading a project saved before mode switching existed.

Track EQ detects pitch from the main input and processes that input. Sidechain
EQ detects pitch from the external Sidechain bus while processing only the
main input. The sidechain is never mixed into the output. The spectrum and
input/output meters continue to represent the main audio path. In Sidechain
EQ, the pitch readout is prefixed with `SC`.

## Controls and persistence

Each mode has an independent bank of eight EQ points, detection algorithm,
Target settings, Tracking, Attack, Release, Gate, Clarity, Mix and Output.
The editor remembers the selected point separately while switching modes.
Bypass and UI Scale are shared. All processing parameters and Operating Mode
are host-automatable and saved in the project.

Track EQ retains the original parameter IDs. Sidechain IDs use the `sc_`
prefix and their host-visible names begin with `Sidechain`. Switching banks
does not copy values or emit parameter changes for the inactive bank.

## Routing and signal behavior

CLAP and VST3 expose stereo and mono layouts with a Sidechain input, as well
as the existing main-only layouts. The default is stereo with a stereo
sidechain. Route a bass or vocal reference to that auxiliary bus in the host.
A host using a main-only layout must select a sidechain-capable layout to
provide external audio.

Missing, silent, below-Gate or otherwise unvoiced sidechain audio produces
no valid pitch; it never changes the detector source to the main input.
The normal Release envelope fades processing out at the last valid frequency.
The editor displays `Waiting for sidechain pitch` when no valid external
pitch is available. This indicates the detection state, not proof that the
host has connected or disconnected the bus.

Changing modes clears the detector window and pending pitch history. Filter
states remain continuous and existing gain/frequency smoothing applies during
the transition. The main path still reports zero samples of latency.

This mode uses fixed EQ gain while voiced. Attack and Release control each
point's processing amount. It does not implement compressor Threshold/Ratio
or gain reduction proportional to sidechain level.

## Verification

DSP tests use different main and sidechain pitches, measure the resulting EQ
gain, check silent-sidechain release and absence of sidechain leakage, verify
main-input spectrum content, and assert allocation-free sidechain processing.
Editor tests switch banks and check independent points and processing values.
The packaged CLAP test checks auxiliary port metadata, routes independent
audio buses through variable-size blocks, automates the mode and saves/restores
both banks through the host ABI.
