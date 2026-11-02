# Liquid Glass appearance sources for macOS Tahoe

Per-appearance layer sets for the VT Code `V` mark, derived from the 120x120 mountain landscape logo.
Each folder holds square, unmasked 1024x1024 PNG layers in back-to-front order:

- `01-background.png` — appearance background (`Clear` is fully transparent).
- `02-V-foreground.png` — glass `V` glyph with transparency.

| Appearance | Background       | Foreground      |
| ---------- | ---------------- | --------------- |
| `Default`  | Mountain sunset  | Glass `V`       |
| `Dark`     | Darkened mountain| Glass `V`       |
| `Clear`    | Transparent      | Glass `V`       |
| `Tinted`   | Monochrome slate | White `V`       |

The shipped `vtcode-app-clear-*.png` / `vtcode-app-tinted-*.png` files are composited from the `Clear` /
`Tinted` layer pairs and then masked with the macOS squircle (see `../README.md` and
`scripts/make-macos-app-icons.py --clear/--tinted`, which accept these folders directly). No
pre-rounded mask is baked into the layer files, so they can also be imported as-is into Apple Icon
Composer (platform macOS, Default/Dark/Mono variants) to tune specular highlights, refraction, and
translucency before exporting a native `.icon` file.

Official docs: <https://developer.apple.com/icon-composer/>
<https://developer.apple.com/design/human-interface-guidelines/app-icons>
