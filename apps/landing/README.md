# PeterFan — Landing Site

The static marketing site for **PeterFan**, a tiny, fast, cross-platform
hardware monitor and fan controller written in Rust.

This is a pure static site with **no build step and no external dependencies** —
no CDNs, remote fonts, or remote images. All icons and graphics are inline SVG
or CSS, and it renders correctly when opened directly via `file://`.

## Files

| File         | Purpose                                              |
| ------------ | ---------------------------------------------------- |
| `index.html` | Page markup and content                              |
| `styles.css` | All styling, theming via CSS custom properties       |
| `script.js`  | Tiny interactions: copy-to-clipboard, counters, reveal |

## Preview

The simplest way:

```sh
# just open it in a browser
open index.html        # macOS
start index.html       # Windows
```

Or serve it locally (recommended, closer to production behavior):

```sh
python3 -m http.server 8080
# then visit http://localhost:8080
```

## Notes

- Dark-themed, fully responsive (mobile + desktop), no horizontal scroll.
- Color palette and spacing are defined as CSS custom properties at the top of
  `styles.css` — adjust there to retheme.
- MIT licensed, same as the project.
