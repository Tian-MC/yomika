# Scrolling performance

The library previously applied a live CSS `filter: blur(18px)` to every
protected cover. On the measured WebKitGTK/NVIDIA configuration, scrolling
caused these filtered layers to be repainted repeatedly. Removing shadows or
changing the top bar did not remove the bottleneck.

Protected covers and decorative game backdrops now use small rasterized
thumbnails. Progressive downsampling and smoothing produce the blur once;
scrolling composites the resulting bitmap. This uses basic Canvas 2D operations
because the tested WebKitGTK does not expose `CanvasRenderingContext2D.filter`.
The cache retains at most 64 bitmaps, each with a maximum dimension of 96 pixels.
Original images remain invisible until an explicit reveal; unavailable canvas
rendering leaves the neutral placeholder. No canvas pixels are read or exported,
so cross-origin VNDB covers and Tauri asset URLs can be rendered without CORS
readback permissions.

## Measurement, 2026-10-29

Synthetic GTK/WebKitGTK 2.54.2 window, 1280 × 820, Wayland/NVIDIA with
`WEBKIT_DISABLE_DMABUF_RENDERER=1` (matching the app's NVIDIA workaround).
The library fixture contained 150 protected covers, all using the same generated
1000 × 1400 SVG image. A five-second requestAnimationFrame loop scrolled down and
back up using the application's CSS. No user library or dialogue data was used.

| Case | Median frame interval | 95th percentile |
| --- | --- | --- |
| Previous CSS blur, repeated baseline | 100–105 ms | 169–174 ms |
| Rasterized covers, first pass with loading/resize observers | 16 ms | 26 ms |
| Rasterized covers, two subsequent passes | 16 ms | 17 ms |

All 150 blurred bitmaps were present in the final measurements. Separate
settings fixtures were already around 16 ms before the change. These are
synthetic frame callback timings on one system, not a universal FPS guarantee
or a measurement of physical input latency.

Tests cover bitmap reuse, bounded cache size, operation without canvas filters,
canvas failure, and keeping original images hidden until explicit reveal.
