# Research notes

Honest write-ups of things we investigated but did **not** ship as features,
so the findings aren't lost and nobody re-walks the same dead ends. PeterFan's
rule is simple: **never present a number we can't stand behind.**

## Apple Silicon GPU utilization (deferred)

**Goal:** a GPU-usage % to sit alongside CPU/memory, matching Activity Monitor.

**What we found.** macOS exposes no public GPU-usage API. The private
**IOReport** framework (`/usr/lib/libIOReport.dylib`) does expose per-power-state
residency. Subscribing to the `GPU Stats` group and diffing two samples yields a
`GPUPH` channel — a histogram over GPU power states:

```
GPUPH states=16 [OFF=2078720, P1=698836, P2=308774, P3=302005, P5=309115, P6..P15=0]
```

The obvious formula, `usage = (total − OFF) / total`, is what asitop/macmon use.
**But it disagrees with Activity Monitor.** At true desktop idle the GPU still
spends ~half its time in a low active P-state (P1) servicing display
compositing, so this reads **~50–70 % when Activity Monitor shows single
digits**. It measures "not fully powered off," not "how hard the GPU is
working."

**Decision.** Not shipped. A faithful number needs a frequency- or
power-weighted residency (weight each P-state by its clock/power, not 0/1), and
we'd want to verify it against ground truth — which on macOS means
`powermetrics`, and that needs root. Until we can both compute *and* verify it,
shipping a figure that visibly contradicts the OS would break the project's
honesty rule.

**Preserved work.** The IOReport plumbing is correct and reusable; it lives in
`packages/platform/src/macos_gpu.rs` behind the off-by-default
`experimental-gpu` Cargo feature:

```sh
cargo run -p peterfan-platform --features experimental-gpu ...
```

Good next step: weight residency by per-state frequency (also readable from
IOReport) and cross-check against `sudo powermetrics --samplers gpu_power`.

## Fan control on Apple Silicon (the `Ftst` unlock)

**Earlier (wrong) conclusion:** we saw `F0Md`/`F0Tg` writes "succeed" with no
physical effect and gated control to Intel only (v0.17). That was a mistake on
two counts:

1. **The writes were never run as root.** The `AppleSMC` user-client rejects
   non-root writes with `kIOReturnNotPrivileged` — and the menu-bar app is
   unprivileged. Real control must go through `sudo`/the root daemon.
2. **Apple Silicon needs an unlock step.** `thermalmonitord` enforces a "System
   Mode" and reverts a bare `F0Md = 1` back to `0` after ~3–4 s, so it looks
   accepted but does nothing. The working sequence (community reverse
   engineering, see refs) is:
   - try `F0Md = 1` (ui8) — enough on M1 and M5;
   - if it doesn't stick, write **`Ftst = 1`** (ui8) to inhibit the thermal
     servo, then re-write `F0Md = 1`, polling ~4 s until it holds;
   - write target RPM to `F0Tg` (little-endian `flt`);
   - restore with `F0Md = 0` then `Ftst = 0`.
   M5 uses a lowercase mode key (`F0md`) and needs no unlock; `Ftst` is absent
   there.

Implemented in `packages/platform/src/smc_write.rs` (v0.26.x). Control is
**attempted on all SMC machines and verified by reading RPM back** — because
some firmware revisions still ignore manual control, we only claim success when
the RPM actually changes.

Refs: exelban/stats #2928 ("Fan control doesn't work on Apple Silicon"),
agoodkind/macos-smc-fan (M1–M5 unlock research), CrystalIDEA Macs Fan Control
release notes (1.5.18 restored control after macOS 14.7/Sequoia firmware
tightening).
