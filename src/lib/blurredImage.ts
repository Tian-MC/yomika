// Rasterize blur once at thumbnail resolution. Keeping a live CSS filter on
// every cover forces WebKitGTK to repaint the library during scrolling.
const MAX_CACHED = 64;
const bitmaps = new Map<string, HTMLCanvasElement>();

export function drawBlurredImage(
  canvas: HTMLCanvasElement,
  image: HTMLImageElement,
  width: number,
  height: number,
  blur: number,
  saturation: number,
): boolean {
  if (width <= 0 || height <= 0 || !image.naturalWidth || !image.naturalHeight) return false;
  const key = JSON.stringify([image.currentSrc || image.src, Math.round(width), Math.round(height), blur, saturation]);
  let bitmap = bitmaps.get(key);
  if (!bitmap) {
    bitmap = document.createElement("canvas");
    const scale = Math.min(1, 96 / Math.max(width, height));
    bitmap.width = Math.max(1, Math.round(width * scale));
    bitmap.height = Math.max(1, Math.round(height * scale));
    // WebKitGTK does not expose CanvasRenderingContext2D.filter on every
    // supported system. Progressive downsampling uses basic canvas operations
    // only, and cannot accidentally fall back to a sharp sensitive image.
    let reduced = document.createElement("canvas");
    reduced.width = bitmap.width;
    reduced.height = bitmap.height;
    const context = reduced.getContext("2d");
    if (!context) return false;
    const zoom = Math.max(reduced.width / image.naturalWidth, reduced.height / image.naturalHeight) * 1.15;
    const w = image.naturalWidth * zoom;
    const h = image.naturalHeight * zoom;
    try {
      context.imageSmoothingEnabled = true;
      context.imageSmoothingQuality = "high";
      context.drawImage(image, (reduced.width - w) / 2, (reduced.height - h) / 2, w, h);
      const samplesX = Math.max(1, Math.min(bitmap.width, Math.floor(width / (blur * 2))));
      const samplesY = Math.max(1, Math.min(bitmap.height, Math.floor(height / (blur * 2))));
      while (reduced.width > samplesX || reduced.height > samplesY) {
        const next = document.createElement("canvas");
        next.width = Math.max(samplesX, Math.floor(reduced.width / 2));
        next.height = Math.max(samplesY, Math.floor(reduced.height / 2));
        const ctx = next.getContext("2d");
        if (!ctx) return false;
        ctx.imageSmoothingEnabled = true;
        ctx.imageSmoothingQuality = "high";
        ctx.drawImage(reduced, 0, 0, next.width, next.height);
        reduced = next;
      }
      const blurred = bitmap.getContext("2d");
      if (!blurred) return false;
      blurred.imageSmoothingEnabled = true;
      blurred.imageSmoothingQuality = "high";
      blurred.drawImage(reduced, 0, 0, bitmap.width, bitmap.height);
      if (saturation < 1) {
        blurred.save();
        blurred.globalCompositeOperation = "saturation";
        blurred.globalAlpha = 1 - saturation;
        blurred.fillStyle = "#808080";
        blurred.fillRect(0, 0, bitmap.width, bitmap.height);
        blurred.restore();
      }
    } catch {
      return false;
    }
    if (bitmaps.size >= MAX_CACHED) bitmaps.delete(bitmaps.keys().next().value!);
    bitmaps.set(key, bitmap);
  } else {
    bitmaps.delete(key);
    bitmaps.set(key, bitmap);
  }
  canvas.width = bitmap.width;
  canvas.height = bitmap.height;
  const output = canvas.getContext("2d");
  if (!output) return false;
  // drawImage works with cross-origin/asset sources without reading pixels or
  // exporting data URLs; CORS-tainted canvases can still be displayed safely.
  try {
    output.drawImage(bitmap, 0, 0);
    return true;
  } catch {
    return false;
  }
}
