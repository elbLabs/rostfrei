# Rostfrei duck

Artwork supplied by the user on 15 October 2026, selected for the Reference ’15
website and Tracer Studio identity.

| File | Purpose |
| --- | --- |
| `duck-original.png` | Original 1254 × 1254 RGBA upload, preserved byte-for-byte. |
| `duck.png` | Cropped full mascot, 512 × 469, transparent background. |
| `duck-dark.png` | Full mascot with a fine neutral silhouette keyline. |
| `duck-mark.png` | Head-and-neck crop, 128 × 128, for the wordmark. |
| `duck-mark-dark.png` | Matching head crop with a dark-surface keyline. |
| `duck-icon.png` | 64 × 64 head crop with a keyline for browser tabs. |

The derivatives retain the illustration's original colors. Disconnected stray
pixels outside the main duck silhouette were removed before cropping (290 small
components, 1,596 pixels); the original upload is retained separately.

The full-art crop is `(x: 82, y: 133, width: 1090, height: 999)` in the original
image. The head crop is `(x: 745, y: 135, width: 432, height: 432)`. Both were
downsampled with high-quality browser canvas interpolation. The dark variants
add a one-pixel alpha dilation behind the scaled artwork, colored `#E2E9EE` at
65% opacity. This keeps the charcoal bill, tail, and feet distinct on dark steel.

The design study uses the full mascot beside the website introduction and the
head crop beside the wordmark in both headers. The image is decorative inside
the named home links; the standalone mascot has descriptive alternative text.
Explicit theme selection controls the `<picture>` source, so the artwork follows
the selected theme even when it differs from the operating-system preference.
