# Security policy

PeterFan includes a privileged macOS fan-control daemon, local IPC, a local HTTP
API and an updater. Report a suspected vulnerability privately before opening a
public issue or pull request with exploit details.

## Reporting a vulnerability

Use GitHub's [private vulnerability report](https://github.com/uulab-official/peterfan/security/advisories/new)
(preferred), or email [uulab.official@gmail.com](mailto:uulab.official@gmail.com)
with the subject `PeterFan security report`. Private reporting is enabled for the
official repository. The email is UULab's public contact and remains an
alternative if GitHub reporting is unavailable. Do not include secrets or
personal machine identifiers.

Include the release version or source commit, operating system and architecture,
affected component, expected and observed behavior, impact, and a minimal
reproduction where possible. Prefer mock data. Remove tokens, signing material,
usernames, serial numbers, IP addresses and local paths from attachments.

Reports are handled on a best-effort basis; there is no guaranteed response time.
Please coordinate disclosure with the maintainers. Do not test against anyone
else's machine or intentionally disable hardware safety mechanisms.

## Supported versions

Security fixes target the latest published release and current development
branch. Older releases do not receive routine backports. Check
[GitHub Releases](https://github.com/uulab-official/peterfan/releases/latest)
before reporting; a newer source checkout is not necessarily a published,
signed release. This project is beta software and has no security audit or
hardware compatibility guarantee.

Builds, mock tests, OS signing/notarization, actual fan behavior and extended
hardware testing provide different evidence. A signed app is not proof that
every Mac model supports manual control. See the
[production checklist](docs/PRODUCTION_CHECKLIST.md).

For ordinary bugs and setup questions, use [SUPPORT.md](SUPPORT.md).
