import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import { drawBlurredImage } from "../lib/blurredImage";

interface Props {
  src: string;
  blur: number;
  saturation: number;
  eager?: boolean;
  fallback?: ReactNode;
  onError?: () => void;
}

/** Displays a cached blurred bitmap. The original stays invisible until reveal. */
export function BlurredImage({ src, blur, saturation, eager, fallback, onError }: Props) {
  const image = useRef<HTMLImageElement>(null);
  const bitmap = useRef<HTMLCanvasElement>(null);
  const [readyFor, setReadyFor] = useState<string | null>(null);
  const key = JSON.stringify([src, blur, saturation]);
  const ready = readyFor === key;

  const draw = useCallback(() => {
    if (!image.current || !bitmap.current) return;
    const rect = image.current.getBoundingClientRect();
    const ok = drawBlurredImage(bitmap.current, image.current, rect.width, rect.height, blur, saturation);
    setReadyFor(ok ? key : null);
  }, [key, blur, saturation]);

  useEffect(() => {
    const img = image.current;
    if (!img) return;
    // Covers may already be decoded when remounting a page or changing theme.
    if (img.complete) draw();
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(draw);
    observer?.observe(img);
    return () => observer?.disconnect();
  }, [draw]);

  return (
    <>
      {!ready && fallback}
      <img ref={image} className="blur-source" style={{ visibility: "hidden" }} src={src} alt="" aria-hidden loading={eager ? "eager" : "lazy"} decoding="async" draggable={false} onLoad={draw} onError={onError} />
      <canvas ref={bitmap} className="blur-bitmap" hidden={!ready} aria-hidden />
    </>
  );
}
