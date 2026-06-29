# PeterFan — Landing Site

The static marketing site for **PeterFan**, a tiny, fast, cross-platform
hardware monitor and fan controller written in Rust.

This is a **single, self-contained `index.html`** with no build step and no
external dependencies — the CSS and JS are inlined, all icons/graphics are
inline SVG or CSS. It renders correctly when opened directly via `file://`, when
served, and in previewers that don't load linked assets.

> The CSS is inlined at the top of `<body>` (not just `<head>`) on purpose, so
> the page still styles correctly in tools that strip or ignore `<head>`.

## Files

| File         | Purpose                                   |
| ------------ | ----------------------------------------- |
| `index.html` | The entire site — markup, inline CSS & JS |

## Preview

```sh
# just open it in a browser
open index.html        # macOS
start index.html       # Windows

# or serve it locally
python3 -m http.server 8080   # then visit http://localhost:8080
```

## Notes

- Light theme modeled on [mac-stats.com](https://mac-stats.com) (Stats):
  Tailwind-style neutral grays, a blue→violet accent, and system fonts. The
  terminal/code mockups stay dark on the light page. Fully responsive (mobile +
  desktop), no horizontal scroll.
- Color palette and spacing are defined as CSS custom properties at the top of
  `styles.css` — adjust there to retheme.
- MIT licensed, same as the project.
