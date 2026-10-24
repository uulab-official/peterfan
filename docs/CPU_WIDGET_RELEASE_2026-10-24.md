# CPU-widget release and installed acceptance — 2026-10-24

Target: **v1.32.8**, including the independent macOS CPU bar/history/number widget.
Release preparation is in progress. Publication, actual public OTA and a new
six-hour installed-build result will be recorded as separate evidence below.

The previous [v1.32.7 six-hour acceptance](LONG_RUN_SAFETY_2026-10-24.md) remains
historical evidence and does not cover this feature or any new build.

## Source and compatibility

- Existing users default to the widget being off. This Mac's requested installed
  acceptance will enable Bar with percentage alongside the existing runner.
- A healthy v1.32.7 fan helper remains compatible. An app-only feature does not
  require another privileged install; QA pins the actual helper version, PID and
  hash separately from the new app.
- Both native status items open the dashboard at the clicked item. Settings and
  the native context menu provide Bar, History, Number and percentage choices.
- Fixed widths, native template rendering, one-second bounded sampling and
  resume-history reset retain the approved feature behavior.

## Acceptance ledger

- [ ] Reviewed exact v1.32.8 source and CI.
- [ ] Universal Developer ID signed/notarized Mac assets and complete provider
      asset/manifest verification, including native Windows artifact acceptance.
- [ ] Public-feed OTA from the installed v1.32.7 app to v1.32.8, with a new
      surviving process, per-launch native readiness and exact artifact bytes.
- [ ] Actual installed widget settings/style/click checks and all unrelated
      settings preserved; compatible helper retains healthy automatic control.
- [ ] New frozen installed-build baseline and complete 21,600-second observation.

Publication alone does not prove installation; calibration is not six-hour
acceptance. Other Mac models, physical sleep/wake and forced installation
interruption remain separate gates. Beta status is retained.
