# Third-party notices

PeterFan source code and original artwork are distributed under the
[MIT License](LICENSE). External artwork below retains its stated **CC0 1.0**
dedication; it is not relabeled as UULab's original MIT artwork. Dependency
crates retain their own licenses, recorded in their packages and Cargo metadata.
`Cargo.lock` records the dependency versions used for product builds.

## Runner PNG files

| Local files in `assets/runner/` | Author / credit | Source and license | Current use |
| --- | --- | --- | --- |
| `dino-run-1-cc0.png` … `dino-run-8-cc0.png` | pzUH | [Free Dino Sprites](https://opengameart.org/content/free-dino-sprites), CC0 | Runtime Dinosaur frames; cropped/scaled/normalized by PeterFan |
| `runner-sheet.png` | David Harrington / Blue Yeti Studios | [Runner character](https://opengameart.org/content/runner-character), CC0 | Historical comparison; Human is now original procedural art |
| `cat-run-cc0.png` | alizard | [pixel cat](https://opengameart.org/content/pixel-cat-0), CC0 | Historical comparison |
| `dog-run-cc0.png` | rmazanek; based on work by Shepardskin and Hellkipz | [Dog](https://opengameart.org/content/dog-3), CC0 | Historical comparison |
| `fox-run-cc0.png`, `rabbit-run-cc0.png` | ScratchIO | [Animated Wild Animals](https://opengameart.org/content/animated-wild-animals), CC0 | Historical comparison |
| `ghost-run-cc0.png` | ImogiaGames; remixed from Ghost and Skeleton by Balmer | [Ghost monster](https://opengameart.org/content/ghost-monster), CC0 | Historical comparison |

The source pages listed above identify these works as CC0; reviewed 2026-10-22.
See the [CC0 legal text](https://creativecommons.org/publicdomain/zero/1.0/legalcode)
and [machine-readable inventory](assets/runner/sources.json) for per-file
identity, source, author and runtime/reference usage. CC0 does not require
attribution; these credits preserve useful provenance.

All other current runner characters are authored in PeterFan's procedural
renderer. See [the runner guide](assets/runner/README.md). RunCat is a behavioral
reference; its artwork is not included in PeterFan.

The former unused `caveman.png` and `correr.png` comparison files were removed
from the current source tree because their source/license were not recorded.
This notice does not assign a license to those files or certify old Git history.

New app bundles keep this notice and the MIT license in `Contents/Resources/`.
New macOS universal archives and Windows ZIPs include both files alongside their
README/changelog. Earlier published releases are not retroactively modified.

## tray-icon 0.24.1

The app includes tray-icon by the Tauri Programme, licensed under MIT or
Apache-2.0. PeterFan distributes this copy under the MIT option below, with a
macOS click-handling backport. Source and patch provenance are retained in
`vendor/tray-icon-0.24.1/README.backport.md`.

MIT License

Copyright (c) 2022-2022 Tauri Programme within The Commons Conservancy

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
