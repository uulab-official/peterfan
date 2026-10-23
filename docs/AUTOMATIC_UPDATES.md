# Automatic update checks

The menu-bar app checks GitHub Releases without opening a dialog or installing
anything in the background. Installation remains an explicit action in Settings,
the tray menu or the CLI, and keeps the existing checksum, signature, platform
identity and macOS notarization checks.

## Check policy

| Event | Next automatic check |
| --- | --- |
| App launch | After 20 seconds |
| Successful discovery | After 6 hours |
| First failed discovery | After 5 minutes |
| Second consecutive failure | After 15 minutes |
| Further consecutive failures | After 1 hour |
| Newer release still uploading verified assets | Same bounded retry schedule; metadata success remains recorded |
| Sleep passes the deadline | Within 30 seconds after the app resumes |
| Manual check | Immediate when no other update action is running; resets the automatic deadline |

A successful discovery resets the failure count. The scheduler uses elapsed wall
clock deadlines and sends one request when overdue; it does not replay missed
intervals after a long sleep. The app must be running to check. Mock mode never
starts real update requests.

Automatic discovery, manual checks, installation and channel changes share one
native update lease. Results are tied to the channel being checked. Changing
channels clears previous release metadata and schedules a fresh check. Failed
channel saves remain failures rather than displaying an unsaved choice.

Settings displays the most recent attempt, last successful check, next automatic
check and retry state. An unavailable network is a failed check, never proof that
the app is current. Malformed release versions are also failed checks. A newer release without complete verified asset metadata is
shown as unavailable for installation; it does not generate a ready-to-install
notification.

Optional notifications are deduplicated per version/channel for the current app
run. Check history and notification deduplication reset when the app restarts.
This does not change the separately persisted installation result receipt.

## Verification boundaries

Deterministic policy tests cover startup, successful/manual rescheduling, bounded
failure retries, overdue wake behavior and mutual exclusion. Dashboard tests run
the actual embedded JavaScript against controlled check/receipt states. Browser
fixtures exercise the Rust-generated settings screen without writing user config.

A real public-feed lookup confirms service access and version/asset parsing. It
does not prove a newer release was downloaded, installed or relaunched. Those
claims require a published newer release, the installed executable's identity and
the detached install receipt to agree.

Windows custom install paths are preserved by passing the running executable's
installation directory to the bundled installer. Windows runtime installation,
launch health and transactional rollback require separate Windows verification;
a generated-script check or cross-compilation is not that evidence.
The installer requires a directory writable by the current user; it does not
request elevation for installations under protected locations such as Program Files.
