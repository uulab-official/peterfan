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

Use this only for a trusted personal release machine.

1. Open Keychain Access on the current release Mac.
2. Find `Developer ID Application: Choi Tae Ho (N99FMBQ662)`.
3. Export it as a password-protected `.p12`.
4. Move the `.p12` to the other Mac through a secure channel.
5. Import it into that Mac's login Keychain.
6. Run `scripts/setup-macos-signing.sh notary` on that Mac and create a local
   `peterfan-notary` profile with a fresh app-specific password.
7. Run `scripts/check-macos-release.sh`.

Never commit or upload the `.p12`. Delete the transfer copy after import.

## Release Flow

From a clean, tagged release commit:

```bash
scripts/release-local-macos.sh vX.Y.Z --draft
```

This builds universal macOS binaries, signs the app, notarizes the app and DMG,
staples tickets, creates checksums, and uploads release assets with `gh`.

For a local dry run without uploading:

```bash
scripts/release-local-macos.sh vX.Y.Z --no-upload
```
