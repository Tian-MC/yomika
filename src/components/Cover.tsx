import { EyeOff } from "lucide-react";
import { memo, useState } from "react";
import type { GameMetadata } from "../contract";
import { useT } from "../i18n";
import { coverSrc, hueFor, shouldBlur } from "../lib/cover";
import { useAppSettings } from "../state/app";

interface CoverProps {
  title: string;
  originalTitle?: string | null;
  metadata: Pick<GameMetadata, "coverPath" | "coverUrl" | "coverSexual">;
  /** Explicit src (e.g. VNDB search result) instead of metadata. */
  src?: string | null;
  size?: "sm" | "md" | "lg" | "fill";
  /** Allow click-to-reveal when blurred. */
  revealable?: boolean;
  eager?: boolean;
}

/** Cover image with lazy loading, graceful placeholder and NSFW blur. */
export const Cover = memo(function Cover({ title, originalTitle, metadata, src, size = "md", revealable, eager }: CoverProps) {
  const t = useT();
  const { appSettings } = useAppSettings();
  const [failed, setFailed] = useState(false);
  const [revealed, setRevealed] = useState(false);
  const url = src !== undefined ? src : coverSrc(metadata);
  const blur = !revealed && shouldBlur(metadata.coverSexual, appSettings?.vndb.blurSexualThreshold);
  const showImage = url && !failed;
  const hue = hueFor(title);
  const initial = (originalTitle || title).trim().slice(0, 1).toUpperCase() || "?";
  return (
    <div className={`cover cover-${size}${blur ? " is-blurred" : ""}`} style={{ "--cover-hue": hue } as React.CSSProperties}>
      {showImage ? (
        <img src={url} alt="" loading={eager ? "eager" : "lazy"} decoding="async" draggable={false} onError={() => setFailed(true)} />
      ) : (
        <div className="cover-placeholder" aria-hidden>
          <span className="cover-initial" lang={originalTitle ? "ja" : undefined}>
            {initial}
          </span>
          <span className="cover-ph-title">{title}</span>
        </div>
      )}
      {blur && showImage && (
        revealable ? (
          <button type="button" className="cover-blur-note" onClick={(e) => { e.preventDefault(); e.stopPropagation(); setRevealed(true); }}>
            <EyeOff size={14} aria-hidden /> {t("cover.reveal")}
          </button>
        ) : (
          <span className="cover-blur-note" title={t("cover.blurred")}>
            <EyeOff size={14} aria-hidden />
            <span className="sr-only">{t("cover.blurred")}</span>
          </span>
        )
      )}
    </div>
  );
});
