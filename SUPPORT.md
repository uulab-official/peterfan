# Getting help

| Need | Start here |
| --- | --- |
| Install or update | [README](README.md#install), [macOS distribution](docs/MACOS_DISTRIBUTION.md), [Windows distribution](docs/WINDOWS_DISTRIBUTION.md) |
| CLI usage | [CLI reference](docs/CLI.md), `peterfan --help`, `peterfan doctor` |
| Reproducible bug | [Bug report](https://github.com/uulab-official/peterfan/issues/new?template=bug_report.yml) |
| Feature proposal | [Feature request](https://github.com/uulab-official/peterfan/issues/new?template=feature_request.yml) |
| Contribute code or documentation | [CONTRIBUTING](CONTRIBUTING.md), [development guide](docs/DEVELOPMENT.md) |
| Security or private conduct concern | [SECURITY](SECURITY.md), [code of conduct](CODE_OF_CONDUCT.md) |

Include the installed version or source commit, OS/architecture, affected
interface and a small reproduction. State whether you used `--mock` or real
hardware. For fan issues include the Mac model (not its serial number), daemon
readiness and expected/observed RPM. Do not deliberately stop fans or disable
critical-temperature protection to reproduce a report.

The app's Fan Control Health **Copy Report** is intended for sharing diagnostic
context. Review any report before posting. CLI JSON and raw logs may contain
local paths or other machine details; redact these and never attach credentials,
signing files, `.env` or the entire configuration directory.

macOS sensor/control capabilities depend on hardware and firmware. Windows
provides real system metrics and firmware ACPI thermal zones when available;
CPU-core temperatures and fan RPM/control are not supported by its current
backend. Linux full desktop support is not claimed; portable core contributors
can use the headless checks in the development guide.

Support and issue triage are best effort. A local build, open pull request or
passing mock test does not mean a release was published or hardware was tested.
