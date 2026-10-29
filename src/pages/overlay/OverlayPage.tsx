import { Eraser, Eye, EyeOff, GripHorizontal, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { OverlayConfig, TranslationLine } from "../../contract";
import { useI18n } from "../../i18n";
import { call } from "../../lib/api";
import { defaultOverlayConfig } from "../../lib/defaults";
import { useEvent } from "../../lib/hooks";

const MAX_LINES = 3;

/**
 * Transparent, chrome-less window (label "overlay") that shows the latest
 * translated lines. Drag anywhere on the background to move it.
 */
export function OverlayPage() {
  const { t } = useI18n();
  const [lines, setLines] = useState<TranslationLine[]>([]);
  const [config, setConfig] = useState<OverlayConfig>(defaultOverlayConfig);
  const [showOriginal, setShowOriginal] = useState<boolean | null>(null);
  const configCache = useRef(new Map<number, OverlayConfig>());
  const lastGame = useRef<number | null>(null);
  /** Set once overlay_show pushed a config; it then wins over per-game lookups. */
  const pushedConfig = useRef(false);

  useEffect(() => {
    const root = document.documentElement;
    root.classList.add("overlay-mode");
    return () => root.classList.remove("overlay-mode");
  }, []);

  // overlay_show sends the effective OverlayConfig to this window; apply it live.
  useEvent("vnl://overlay-config", (cfg) => {
    pushedConfig.current = true;
    setConfig(cfg);
    setShowOriginal(null);
  });

  useEvent("vnl://translation", (line) => {
    setLines((cur) => [...cur.filter((l) => l.id !== line.id), line].slice(-MAX_LINES));
    const gameId = line.gameId;
    if (!pushedConfig.current && gameId !== null && gameId !== lastGame.current) {
      lastGame.current = gameId;
      const cached = configCache.current.get(gameId);
      if (cached) setConfig(cached);
      else
        call("library_get_settings", { gameId })
          .then((s) => {
            configCache.current.set(gameId, s.translation.overlay);
            if (lastGame.current === gameId && !pushedConfig.current) setConfig(s.translation.overlay);
          })
          .catch(() => undefined);
    }
  });

  const original = showOriginal ?? config.showOriginal;
  const latest = lines[lines.length - 1];

  return (
    <div className="overlay-root" data-tauri-drag-region>
      <div className="overlay-panel" data-tauri-drag-region style={{ fontSize: config.fontSize, background: `rgba(10, 12, 18, ${config.opacity})` }}>
        <div className="overlay-controls">
          <span className="overlay-grip" data-tauri-drag-region aria-hidden>
            <GripHorizontal size={14} />
          </span>
          <button type="button" aria-label={original ? t("overlay.hideOriginal") : t("overlay.showOriginal")} title={original ? t("overlay.hideOriginal") : t("overlay.showOriginal")} onClick={() => setShowOriginal(!original)}>
            {original ? <EyeOff size={14} /> : <Eye size={14} />}
          </button>
          <button type="button" aria-label={t("overlay.clear")} title={t("overlay.clear")} onClick={() => setLines([])}>
            <Eraser size={14} />
          </button>
          <button type="button" aria-label={t("overlay.close")} title={t("overlay.close")} onClick={() => void call("overlay_hide", {}).catch(() => undefined)}>
            <X size={14} />
          </button>
        </div>
        {!latest ? (
          <p className="overlay-waiting" data-tauri-drag-region>
            {t("overlay.waiting")}
          </p>
        ) : (
          <div className="overlay-lines" aria-live="polite" data-tauri-drag-region>
            {lines.map((l, i) => (
              <div key={l.id} className={i === lines.length - 1 ? "overlay-line is-latest" : "overlay-line"} data-tauri-drag-region>
                {original && (
                  <div className="ov-original" lang={l.sourceLanguage ?? "ja"}>
                    {l.sourceText}
                  </div>
                )}
                {l.translatedText ? (
                  <div className="ov-translated" lang={l.targetLanguage ?? undefined}>
                    {l.translatedText}
                  </div>
                ) : l.error ? (
                  <div className="ov-error">{l.error}</div>
                ) : null}
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
