# Homebrew packaging

PeterFan ships two Homebrew artifacts with separate responsibilities:

- `peterfan.rb` is the CLI Formula for terminal users.
- `Casks/peterfan.rb` installs the signed macOS `PeterFan.app` from the official
  universal DMG.

The cask checksum must come from the exact final DMG uploaded to the GitHub
Release. After the local release script has produced and notarized the artifact,
run:

```sh
scripts/update-homebrew-cask.sh \
  v1.30.0 \
  dist/local-release/v1.30.0/PeterFan-v1.30.0.dmg
scripts/check-homebrew.sh
```

The repository stores the cask source so it can be copied into the
`uulab-official/homebrew-peterfan` tap. A public tap release must use the
notarized DMG checksum, not the checksum of a local ad-hoc build.

For local syntax and version checks:

```sh
scripts/check-homebrew.sh
```
