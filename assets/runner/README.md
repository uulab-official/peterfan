# Runner artwork and provenance

PeterFan's 20-character picker uses nineteen original procedural runners plus
an external Dinosaur cycle. Cat, Dog, Rabbit, Fox, Penguin, Robot, Ghost, Human
and Push-up are implemented in `packages/menubar/src/runner/procedural.rs`;
Bird, Duck, Sheep, Fish and Slime in `playful_animals.rs`; Orbit, Sushi, City,
Sausage and Party in `playful_objects.rs`. Original artwork/code is MIT licensed.
No RunCat artwork is bundled.

Only `dino-run-1-cc0.png` through `dino-run-8-cc0.png` are currently embedded by
`runner/assets.rs`. The other listed PNGs are historical comparison material,
not active runtime character sheets. In particular, Human no longer decodes
`runner-sheet.png`.

See [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) for source pages,
authors, license boundaries and upstream derivative credits. The corresponding
machine-readable inventory is [sources.json](sources.json); it records each
retained PNG's SHA-256 and whether it is used at runtime. Source-page CC0 labels
were checked on 2026-10-22. The inventory records current file identity; it does
not claim that a transformed crop is byte-identical to an upstream archive.

New external assets need a source, author, license, intended usage and inventory
hash before merging. `node scripts/check-oss.mjs` checks inventory coverage,
file hashes and embedded-asset usage. Unknown-origin comparison files were
removed from the current tree instead of assigning them an assumed license.
