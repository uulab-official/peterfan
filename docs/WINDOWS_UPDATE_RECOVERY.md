# Windows interrupted update recovery

Existing executable paths remain present while an update replaces their contents.
Before changing any installed file, the trusted helper flushes verified copies of
the original managed files and publishes a transaction journal beside the app.
Existing targets use atomic `File.Replace`; newly introduced files use `Move`.
The stable installation lock covers both normal installation and recovery.

On the next helper invocation, an existing journal is settled before accepting
another update. Recovery needs neither the original download nor the network.
An interrupted apply or rollback restores originals from immutable copies and
removes new files only when their contents match the recorded replacement hash.
Restoration can itself be interrupted and retried. A committed journal verifies
the installed replacement and finishes cleanup instead of rolling it back.

Journal input is bounded and bound to the current installation. Transaction IDs,
phases, required filenames and SHA-256 values are validated; snapshot/stage paths
are derived from the validated ID and fixed filename allowlist. Recovery checks
all relevant files before modifying them. Unknown contents, unsafe paths,
reparse points, missing originals or invalid metadata stop recovery and retain
the journal and snapshots for inspection.

Startup still requires the fresh tray/WebView readiness acknowledgement and
process survival. The installed result precedes the committed marker; backups
are removed only after commit. Receipt publication failure remains a rollback
condition. Cleanup failures retain replayable state for another invocation.

## Verification

`scripts/test-windows-update-recovery.ps1` executes the embedded helper in private
fixture directories, substitutes only startup actions and an explicit barrier,
then hard-kills the exact child process. It replays interrupted apply, rollback,
commit and cleanup states twice, checks original/replacement hashes and unrelated
data, and rejects corrupt journals and snapshots. The Windows transaction driver
also exercises real synthetic executable startup and handled failure paths.

Portable PowerShell execution verifies file/replay behavior; native Windows CI
is required for Windows file sharing, process and executable behavior. Neither
proves a physical power cut or a signed end-user desktop upgrade.

## Operational limits

Recovery runs on the next updater helper invocation. It does not install a boot
task, change registry integration, or promise unattended recovery after reboot.
Close the target installation's app and CLI before retrying. A recovery that
restores the previous version exits after relaunching it; retry the update again
to begin a new transaction. Atomic replacement applies per file, so an interrupted
multi-file update can temporarily contain mixed versions until recovery runs.
Termination before journal publication leaves the original installation intact
but can leave an orphan staging/snapshot directory; recovery does not delete
unreferenced directories speculatively.

Disk flushes improve durability but do not guarantee behavior under every
filesystem, hardware failure or uncooperative disk cache. macOS interruption
recovery is a separate acceptance item. Public release and actual OTA adoption
must be verified independently from these fixtures.
