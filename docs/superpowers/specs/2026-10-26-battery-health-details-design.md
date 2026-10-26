# Battery health and capacity in hardware information

Date: 2026-10-26
Status: User approved design and implementation on 2026-10-26

## Intent and chosen approach

The user wants to see how much capacity the battery retains compared with when
it was new, alongside its current charge. They selected **all information in the
existing battery area** over a summary with expandable capacity/cycle details.
Keep the existing hardware information screen; no new page or menu-bar item.

## Visible information

- Current charge: current charging percentage and charging/power state.
- Battery health / maximum capacity: capacity relative to a new battery, with
  an explicit estimate label if it is calculated rather than OS reported.
- Design capacity: original rated capacity, with its verified unit.
- Full charge capacity: how much the battery can currently store when full,
  in the same unit as design capacity. This is distinct from charge remaining.
- Cycle count: the reported charge-cycle count.

Use compact Korean/English rows in the battery area. Keep charge and health
separate. No prediction of remaining years, replacement date, or lifetime from
cycle count alone. A battery-free desktop shows “No battery”; an unavailable
individual reading shows “Unavailable”, never 0 or a fabricated percentage.

## Data correctness

Reuse the existing monitor and slow dashboard sampling. Add optional structured
capacity/health data without a new sampler, worker, or per-frame subprocess.
On macOS, use native battery data with explicit unit/provenance validation;
prefer trustworthy OS health reporting. A derived percentage may use only
compatible full/design capacities and must be identified as an estimate.
Do not divide normalized charge percentages by physical capacities.

The current host illustrates the hazard: top-level MaxCapacity is 100 while
physical capacity fields are nested under BatteryData. The existing battery
crate health path already rejects some implausible Apple Silicon results.
Confirm the new reading against macOS reporting; do not remove validation just
to obtain a number. Unsupported platforms retain existing readings and expose
unsupported new fields as unavailable.

## Preservation and verification

Preserve fan control, helper compatibility, preferences, menu-bar display,
current installed app, and all signed release artifacts. This is a subsequent
source change; immutable v1.32.10 artifacts must not be replaced.

Verify valid/missing/zero/nonfinite/mismatched-unit readings, charge versus
health distinction, and desktop behavior. Check Korean/English, light/dark,
320/390-width hardware layouts using CUA and the final installed app when
accessible. Fixture evidence remains separate from installed-client evidence.
Use actual new-version signing/OTA and a fresh full observation for any final
release acceptance; older uptime does not validate this feature.

## Approval boundary

The user selected the inline all-details approach and explicitly approved
implementation of this document. Implementation planning and execution follow;
the installed client and immutable v1.32.10 release remain preserved during work.
