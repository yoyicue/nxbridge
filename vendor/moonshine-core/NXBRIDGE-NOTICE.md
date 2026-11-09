# Experimental GameStream protocol core provenance

This is the `vendor/moonshine/moonshine-core` subtree of
[rgsp-cast](https://github.com/codebutler/rgsp-cast/tree/2eeb0fa63221a9e72dfc51ea77a01a3be8ff0e1e),
revision `2eeb0fa63221a9e72dfc51ea77a01a3be8ff0e1e`, which the project
identifies as based on Moonshine v0.15.0. The subtree retains Moonshine's
BSD-2-Clause license in `LICENSE.upstream`; the rgsp-cast root is GPL-3.0
and its license is reproduced in `LICENSE.rgsp-cast`. The rgsp-cast README at
the pinned revision lists its vendored `vendor/moonshine/` tree as BSD-2-Clause;
this declaration was checked at the pinned revision for the source preview.
The retained upstream material remains BSD-2-Clause. NXBridge's integration
changes are distributed under GPL-3.0-only, continuing the project's existing
conservative distribution policy. The package's `BSD-2-Clause AND GPL-3.0-only`
expression records these combined obligations; it does not remove the upstream
BSD grant or claim that the parent project's GPL automatically relicenses it.
This is not the v0.16.1 Moonshine research revision in ADR-0002.

Local modifications: an explicit directory parameter for pairing state and a
5 ms per-packet RTP audio timestamp in `src/session/stream/audio/encoder.rs`
to match Moonlight's audio FEC block invariant. The loopback probe's temporary
network-lookup tracing was removed. See `docs/reports/NXB-032.md` for evidence.

The video packetizer now uses the Moonlight nanors Cauchy parity matrix and
sets parity count using the receiver's rounding rule. The GameStream session
input, recovery, audio and video hooks are local experimental adaptations.
The app list includes an optional NXBridge SHA-256 cover-version extension;
the standard app asset endpoint continues serving the configured PNG bytes.
An optional library-game identity extension associates client layouts with a
stable catalog ID without changing the GameStream application ID. The input handoff now tags bounded-queue events with an epoch, signals overflow and control-peer disconnects, and uses authenticated control activity for an experimental input-silence watchdog. These changes are not an upstream compatibility or latency guarantee.

Audio integration carries source monotonic timestamps, session identity and
discontinuity flags, and bounds gap concealment to four 5 ms silent blocks when
there is no backlog. The encoder owns the output pacer and expires queued PCM
before assigning RTP sequence numbers. One acknowledged data/FEC batch may be
outstanding at the UDP sender, preventing a separate encoded-audio backlog.
Cancellation interrupts both queue and send-completion waits.

Video FEC cache construction and packetization execute as one awaited blocking
job per stream, preserving packetizer state/order without blocking async audio
I/O. Device-free probes cover sender stalls, local datagram scheduling, real
UDP Opus decoding and two-packet FEC recovery. See NXB-107 for measured results
and the outstanding game/client acceptance limits.

NXB-111 adds a bounded, process-local cache of immutable Cauchy encoders. It
uses the local fec-rs direct-matrix constructor to avoid building and inverting
a Vandermonde matrix that would immediately be replaced. Session keys and
counters remain private. Video has one queued encoded frame and one locally
acknowledged packet batch; encoded reference frames are never discarded to
catch up. Both Cargo entry points select the same patched fec-rs package.

The diagnostic `tests/fixtures/input-stage-instrumentation.patch` includes this modified protocol core and follows the same combined license obligations; it is not part of the production runtime.
