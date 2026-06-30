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

## Fan control on Apple Silicon (shipped as Intel-only)

Recorded here for completeness — see `CHANGELOG.md` v0.17.0. SMC fan-control
writes (`F0Md`/`F0Tg`) are accepted on Apple Silicon but have no physical
effect (fans are system-governed; the `FS! ` override key is absent). We
verified fans *do* respond to the OS under load, just not to manual SMC writes,
so fan **control** is gated to Intel Macs while fan **monitoring** works
everywhere.
