# Rostfrei duck

Artwork supplied by the user on 15 October 2026, selected for the Reference ’15
website and Tracer Studio identity. Copied from `website/public/brand` for shared
production consumption; the original upload is preserved byte-for-byte.

| File | Purpose |
| --- | --- |
| `duck-original.png` | Original 1254 × 1254 RGBA upload. |
| `duck.png` | Cropped full mascot, 512 × 469, transparent background. |
| `duck-dark.png` | Full mascot with a fine neutral silhouette keyline. |
| `duck-mark.png` | Head-and-neck crop, 128 × 128, for the wordmark. |
| `duck-mark-dark.png` | Matching head crop with a dark-surface keyline. |
| `duck-icon.png` | 64 × 64 head crop with a keyline for browser tabs. |

Derivatives retain the illustration's original colors. Disconnected stray
pixels outside the main duck silhouette were removed before cropping (290 small
components, 1,596 pixels); the original upload is retained separately.

The full-art crop is `(x: 82, y: 133, width: 1090, height: 999)` in the original
image. The head crop is `(x: 745, y: 135, width: 432, height: 432)`. Both were
downsampled with high-quality browser canvas interpolation. Dark variants add a
one-pixel alpha dilation behind the scaled artwork, colored `#E2E9EE` at 65%
opacity so the charcoal bill, tail, and feet remain distinct on dark steel.

The shared components select light/dark artwork with explicit theme classes in
`styles.css`, rather than OS-only media queries. `new URL(..., import.meta.url)`
lets Vite bundle the assets correctly at any deployment base.
