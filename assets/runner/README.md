# Runner character asset

Cat, Dog, Rabbit, Fox, Penguin, Robot and Ghost now use original PeterFan
authored eight-pose silhouettes in `runner/procedural.rs`. Their bodies retain
volume across poses while limbs, ears, tails or cloth articulate. No RunCat
artwork is bundled. Push-up uses an original linked-joint model with fixed
hand/toe contacts and a rigid spine, with the shared CPU cadence. The historical CC0 sheets below remain as comparison
assets; only Human and Dinosaur currently decode external sprite assets.

The full 20-character picker also includes original Bird, Duck, Sheep, Fish and
Slime cycles in `runner/playful_animals.rs`, plus Orbit, Sushi, City, Sausage
and Party in `runner/playful_objects.rs`. Each has eight distinct poses across
four load bands. Swimming, bouncing, dancing and miniature scenes use their
shared CPU-dependent cadence (20–180 ms per pose) without per-character speed caps.

`runner-sheet.png` is the active eight-frame side-view `Runner Character`
sheet from OpenGameArt:

- Source: <https://opengameart.org/content/runner-character>
- License: CC0; attribution is not required

PeterFan uses the middle eight 32×32 cells for the `Human` menu-bar runner
character. The upper and lower rows are idle/jump references and are
intentionally not part of the runtime cycle.

`cat-run-cc0.png` is the historical five-frame side-view cat run by alizard
from OpenGameArt (retained for comparison):

- Source: <https://opengameart.org/content/pixel-cat-0>
- Direct asset: <https://opengameart.org/sites/default/files/cat_run.png>
- License: CC0; attribution is not required

`caveman.png` and `correr.png` are retained for comparison only.

`dog-run-cc0.png` is the six-frame `Run` row from rmazanek's six-row dog sheet:

- Source: <https://opengameart.org/content/dog-3>
- Direct asset: <https://opengameart.org/sites/default/files/dog_medium.png>
- License: CC0; attribution is not required

`fox-run-cc0.png` and `rabbit-run-cc0.png` are the six-frame `Run` sheets from
ScratchIO's animated wild animals pack:

- Source: <https://opengameart.org/content/animated-wild-animals>
- Direct assets: <https://opengameart.org/sites/default/files/Fox.zip> and
  <https://opengameart.org/sites/default/files/Rabbit.zip>
- License: CC0; attribution is not required

`ghost-run-cc0.png` is ImogiaGames' six-frame side-view Ghost monster cycle:

- Source: <https://opengameart.org/content/ghost-monster>
- Direct asset: <https://opengameart.org/sites/default/files/anim-nme-ghost.png>
- License: CC0; attribution is not required

For animation-quality reference, the project also reviewed OpenGameArt's CC0
`Runner Character` (eight 32×32 side-scroll frames) and Kenney's CC0 Blocky
Characters pack. These are reference sources for pose timing and silhouette
consistency; no files from those pages are bundled unless listed above:

- OpenGameArt reference: <https://opengameart.org/content/runner-character>
- Kenney reference: <https://kenney.nl/assets/blocky-characters>
- Both pages were checked for their stated CC0 licensing before use as references.

`dino-run-1-cc0.png` through `dino-run-8-cc0.png` are the eight side-view run
frames from pzUH's Free Dino Sprites pack:

- Source: <https://opengameart.org/content/free-dino-sprites>
- Direct asset: <https://opengameart.org/sites/default/files/FreeDinoSprite.zip>
- License: CC0; attribution is not required
