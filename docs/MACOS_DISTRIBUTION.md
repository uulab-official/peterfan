# macOS Distribution Notes

PeterFan can be distributed from any web host or GitHub Release once the DMG is
signed with Developer ID, notarized by Apple, and stapled.

The finished DMG is public. The signing material that creates it is local-only.

## Public Release Artifact

This file can be uploaded anywhere:

```text
dist/local-release/vX.Y.Z/PeterFan-vX.Y.Z.dmg
```

Before publishing, verify:

```bash
scripts/check-macos-release.sh dist/local-release/vX.Y.Z/PeterFan-vX.Y.Z.dmg
```

Expected trust result:

```text
accepted
source=Notarized Developer ID
```

Expected DMG layout checks:

```text
DMG volume name is PeterFan
DMG filesystem is HFS+
DMG app bundle id is kr.co.uulab.peterfan
Gatekeeper accepts the app inside the DMG
```

Users can install it on other Macs by opening the DMG and dragging
`PeterFan.app` to Applications. Fan control still requires the one-time root
helper installation on each Mac because SMC writes require administrator
privileges.

## Local-Only State

Keep these on the release Mac only:

- `.env`
- `private/`
- the Developer ID Application certificate and private key in Keychain
- the `peterfan-notary` notarytool profile in Keychain
- `dist/` build outputs before upload

These are intentionally ignored by git. Do not commit or upload them.

Recommended project values:

```text
PETERFAN_BUNDLE_ID=kr.co.uulab.peterfan
PETERFAN_DAEMON_LABEL=kr.co.uulab.peterfan.daemon
PETERFAN_LOGIN_ITEM_LABEL=kr.co.uulab.peterfan.menubar
APPLE_TEAM_ID=N99FMBQ662
NOTARYTOOL_PROFILE=peterfan-notary
```

The app-specific password is not stored in the repo. It is stored only inside
the macOS Keychain profile created by `notarytool store-credentials`.

## Repo-Managed State

Commit these so the flow is reproducible:

- `.env.example`
- `scripts/load-env.sh`
- `scripts/setup-macos-signing.sh`
- `scripts/sign-macos.sh`
- `scripts/notarize-macos.sh`
- `scripts/release-local-macos.sh`
- `scripts/check-macos-release.sh`
- `scripts/bundle-macos.sh`
- `scripts/make-dmg.sh`
- `packaging/kr.co.uulab.peterfan.daemon.plist`

## One-Time Setup On A Release Mac

There are two supported ways to make a Mac a release machine.

### Option A: Create a new Developer ID certificate on that Mac

This is the preferred path for a second release Mac because the private key is
created locally and never leaves that machine.

```bash
cp .env.example .env
scripts/setup-macos-signing.sh teams
scripts/setup-macos-signing.sh csr
```

Upload `private/macos-signing/CertificateSigningRequest.certSigningRequest` to
Apple Developer as a Developer ID Application certificate. Download the `.cer`,
then import it:

```bash
scripts/setup-macos-signing.sh import /path/to/developerID_application.cer
scripts/setup-macos-signing.sh notary
scripts/check-macos-release.sh
```

Set these local values in `.env`:

```text
PETERFAN_SIGN_IDENTITY="Developer ID Application: Choi Tae Ho (N99FMBQ662)"
APPLE_ID=you@example.com
APPLE_TEAM_ID=N99FMBQ662
NOTARYTOOL_PROFILE=peterfan-notary
```

### Option B: Move the existing certificate to another Mac

Use this only for a trusted personal release machine. A `.cer` by itself is
not enough: the exported archive must contain the Developer ID certificate and
its matching private key.

1. On the current release Mac, open Keychain Access and locate
   `Developer ID Application: Choi Tae Ho (N99FMBQ662)`.
2. Export the identity as a password-protected `.p12`. Use a different secure
   channel for the archive and its password; do not put either in chat, email,
   git, or a shared download folder.
3. On the destination Mac, clone the same tagged source and create `.env` from
   `.env.example`. Set `PETERFAN_BUNDLE_ID`, `APPLE_TEAM_ID`, and the expected
   `PETERFAN_SIGN_IDENTITY` value locally.
4. Import the archive into the destination user's login Keychain. The helper
   prompts for the `.p12` password and never accepts it as a command-line
   argument:

   ```bash
   scripts/setup-macos-signing.sh import-p12 /secure-transfer/PeterFan-Developer-ID.p12
   ```

5. Create a new local notarization profile on the destination Mac. Do not try
   to copy a Keychain profile from the old Mac:

   ```bash
   scripts/setup-macos-signing.sh notary
   ```

   Use a fresh app-specific password or an App Store Connect API key. The
   profile name may remain `peterfan-notary`, but its secret stays in the new
   Mac's Keychain.
6. Verify the imported identity, Team ID, bundle ID, and notary profile before
   building. If a bundle already exists, the command verifies its signature too:

   ```bash
   scripts/setup-macos-signing.sh verify dist/PeterFan.app
   ```

7. Run a no-upload signed dry run, then validate the final DMG:

   ```bash
   scripts/release-local-macos.sh vX.Y.Z --no-upload --skip-windows
   scripts/check-macos-release.sh dist/local-release/vX.Y.Z/PeterFan-vX.Y.Z.dmg
   ```

8. Delete the `.p12` from the transfer location and empty the Trash on both
   Macs. Keep only the Keychain identity and the notary profile on the
   authorized destination Mac. If a backup is required, store it in the
   organization's encrypted secrets vault, never in this repository.

Never commit or upload the `.p12`, private key, app-specific password, API key,
or `.env`. If the identity cannot be exported safely, use Option A and create a
new Developer ID certificate on the destination Mac instead.

### Destination Mac verification checklist

Before allowing the destination Mac to publish a release, all of these must
pass without printing credentials:

```text
Developer ID identity is present and matches the configured Team ID
PETERFAN_BUNDLE_ID is kr.co.uulab.peterfan
notarytool can read the local peterfan-notary Keychain profile
an existing PeterFan.app is signed with the expected identity, when supplied
the final DMG passes signature, stapling, bundle ID, and Gatekeeper checks
```

The `verify` subcommand performs the first four checks. The final DMG check is
intentionally separate because it requires a completed notarization ticket.

## Release Flow

From a clean, tagged release commit:

```bash
scripts/release-local-macos.sh vX.Y.Z --draft
```

This builds universal macOS binaries, signs the app, notarizes the app and DMG,
staples tickets, creates checksums, and uploads release assets with `gh`.

The in-app updater keeps its last bounded result at
`~/Library/Application Support/PeterFan/update-result.json`. The replacement
app reads this record to show whether installation completed, rolled back to
the previous app, or failed before replacement. It contains no credentials or
hardware data and is overwritten by the next OTA attempt.

For a local dry run without uploading:

```bash
scripts/release-local-macos.sh vX.Y.Z --no-upload
```
