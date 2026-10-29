import { afterEach, describe, expect, it, vi } from "vitest";
import { drawBlurredImage } from "./blurredImage";

afterEach(() => vi.restoreAllMocks());

function image(src: string) {
  const img = new Image();
  img.src = src;
  Object.defineProperties(img, { naturalWidth: { value: 500 }, naturalHeight: { value: 700 } });
  return img;
}

function contexts() {
  const created: Array<{ canvas: HTMLCanvasElement; drawImage: ReturnType<typeof vi.fn>; save: ReturnType<typeof vi.fn>; restore: ReturnType<typeof vi.fn>; fillRect: ReturnType<typeof vi.fn> }> = [];
  const map = new WeakMap<HTMLCanvasElement, typeof created[number]>();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockImplementation(function (this: HTMLCanvasElement) {
    let context = map.get(this);
    if (!context) {
      context = { canvas: this, drawImage: vi.fn(), save: vi.fn(), restore: vi.fn(), fillRect: vi.fn() };
      map.set(this, context);
      created.push(context);
    }
    return context as unknown as CanvasRenderingContext2D;
  });
  return created;
}

describe("blurred image rasterization", () => {
  it("reuses a small blurred bitmap without exporting cross-origin pixels", () => {
    const created = contexts();
    const exporting = vi.spyOn(HTMLCanvasElement.prototype, "toDataURL");
    const img = image("https://t.vndb.org/cache-test.jpg");
    const first = document.createElement("canvas");
    const second = document.createElement("canvas");
    expect(drawBlurredImage(first, img, 200, 280, 18, 0.7)).toBe(true);
    const builtContexts = created.length;
    expect(drawBlurredImage(second, img, 200, 280, 18, 0.7)).toBe(true);
    expect(Math.max(first.width, first.height)).toBeLessThanOrEqual(96);
    expect(created).toHaveLength(builtContexts + 1); // only a new output, no new blur
    const originalDraws = created.flatMap((ctx) => ctx.drawImage.mock.calls).filter((call) => call[0] === img);
    expect(originalDraws).toHaveLength(1);
    const firstOutput = created.find((ctx) => ctx.canvas === first)!;
    const secondOutput = created.find((ctx) => ctx.canvas === second)!;
    expect(firstOutput.drawImage.mock.calls[0]?.[0]).toBe(secondOutput.drawImage.mock.calls[0]?.[0]);
    expect(exporting).not.toHaveBeenCalled();
  });

  it("works without canvas filters and never copies a sharp original to the output", () => {
    const created = contexts(); // deliberately has no filter property
    const output = document.createElement("canvas");
    const img = image("/webkit-without-canvas-filter.jpg");
    expect(drawBlurredImage(output, img, 200, 280, 18, 0.7)).toBe(true);
    const visible = created.find((ctx) => ctx.canvas === output)!;
    expect(visible.drawImage.mock.calls[0]?.[0]).toBeInstanceOf(HTMLCanvasElement);
    expect(visible.drawImage.mock.calls[0]?.[0]).not.toBe(img);
    expect(created.some((ctx) => ctx.canvas.width <= 5 && ctx.canvas.height <= 7)).toBe(true);
  });

  it("keeps the protected cover hidden when canvas is unavailable", () => {
    vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
    expect(drawBlurredImage(document.createElement("canvas"), image("/no-canvas.jpg"), 200, 280, 18, 0.7)).toBe(false);
  });

  it("waits for measurable, decoded images", () => {
    const get = vi.spyOn(HTMLCanvasElement.prototype, "getContext");
    expect(drawBlurredImage(document.createElement("canvas"), image("/hidden.jpg"), 0, 0, 18, 0.7)).toBe(false);
    expect(get).not.toHaveBeenCalled();
  });

  it("evicts old thumbnails instead of retaining the whole library", () => {
    const created = contexts();
    const canvas = document.createElement("canvas");
    for (let n = 0; n < 65; n++) {
      expect(drawBlurredImage(canvas, image(`/bounded-${n}.jpg`), 200, 280, 18, 0.7)).toBe(true);
    }
    const draws = () => created.flatMap((ctx) => ctx.drawImage.mock.calls).filter((call) => call[0] instanceof HTMLImageElement).length;
    const before = draws();
    expect(drawBlurredImage(canvas, image("/bounded-0.jpg"), 200, 280, 18, 0.7)).toBe(true);
    expect(draws()).toBe(before + 1);
  });
});
