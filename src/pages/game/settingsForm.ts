import { useCallback, useState } from "react";
import { validateGraphics, type Errors } from "../../components/GraphicsEditor";
import type { AppError, Game, GameSettings } from "../../contract";
import type { TFunction } from "../../i18n";
import { call, normalizeError } from "../../lib/api";
import { useCommand } from "../../lib/hooks";

export interface FilesDraft {
  exePath: string;
  workingDir: string | null;
  launchArgs: string[];
}

export const ENV_KEY_RE = /^[A-Za-z_][A-Za-z0-9_]*$/;
export const DLL_VALUE_RE = /^(n|b|n,b|b,n|)$/;

function filesOf(g: Game): FilesDraft {
  return { exePath: g.exePath, workingDir: g.workingDir, launchArgs: g.launchArgs };
}

export function validateGameSettings(s: GameSettings, files: FilesDraft | null, t: TFunction): Errors {
  const errors: Errors = {};
  const rt = s.runtime;
  if (files) {
    if (!files.exePath.trim()) errors["files.exePath"] = t("validation.required");
  }
  if (rt.prefix.type === "shared" && !rt.prefix.id) errors["runtime.prefix"] = t("validation.choosePrefix");
  if (rt.prefix.type === "custom") {
    if (!rt.prefix.path.trim()) errors["runtime.prefix"] = t("validation.required");
    else if (!rt.prefix.path.startsWith("/")) errors["runtime.prefix"] = t("validation.absolutePath");
  }
  for (const k of Object.keys(rt.env)) {
    if (!ENV_KEY_RE.test(k)) {
      errors["runtime.env"] = t("validation.envKey", { key: k });
      break;
    }
  }
  for (const [k, v] of Object.entries(rt.dllOverrides)) {
    if (!DLL_VALUE_RE.test(v.replace(/\s+/g, ""))) {
      errors["runtime.dllOverrides"] = t("validation.dllValue", { key: k });
      break;
    }
  }
  if (rt.locale.timezone !== null && !/^[A-Za-z_+\-0-9/]+$/.test(rt.locale.timezone)) errors["runtime.locale.timezone"] = t("validation.timezone");
  rt.auxiliary.forEach((a, i) => {
    if (!a.name.trim()) errors[`runtime.auxiliary.${i}.name`] = t("validation.required");
    if (!a.exePath.trim()) errors[`runtime.auxiliary.${i}.exePath`] = t("validation.required");
    if (!Number.isInteger(a.delayMs) || a.delayMs < 0) errors[`runtime.auxiliary.${i}.delayMs`] = t("validation.nonNegativeInt");
  });
  Object.assign(errors, validateGraphics(s.graphics, "graphics", t));
  const tr = s.translation;
  if (!Number.isInteger(tr.hook.attachDelayMs) || tr.hook.attachDelayMs < 0) errors["translation.hook.attachDelayMs"] = t("validation.nonNegativeInt");
  if (!Number.isInteger(tr.ocr.intervalMs) || tr.ocr.intervalMs < 0) errors["translation.ocr.intervalMs"] = t("validation.nonNegativeInt");
  if (tr.ocr.region && (tr.ocr.region.width <= 0 || tr.ocr.region.height <= 0)) errors["translation.ocr.region"] = t("validation.regionSize");
  if (tr.overlay.fontSize < 8 || tr.overlay.fontSize > 96) errors["translation.overlay.fontSize"] = t("validation.range", { min: 8, max: 96 });
  if (tr.overlay.opacity < 0.1 || tr.overlay.opacity > 1) errors["translation.overlay.opacity"] = t("validation.range", { min: 0.1, max: 1 });
  return errors;
}

export interface GameSettingsForm {
  loading: boolean;
  loadError: AppError | null;
  reload: () => Promise<unknown>;
  saved: GameSettings | null;
  draft: GameSettings | null;
  setDraft: (updater: (d: GameSettings) => GameSettings) => void;
  files: FilesDraft | null;
  setFiles: (updater: (f: FilesDraft) => FilesDraft) => void;
  dirty: boolean;
  errors: Errors;
  saving: boolean;
  saveError: AppError | null;
  save: () => Promise<boolean>;
  revert: () => void;
}

/** Load → edit → validate → save/revert state for a game's settings (plus exe/working dir/args). */
export function useGameSettingsForm(gameId: number, game: Game | undefined, onGameSaved: (g: Game) => void, t: TFunction): GameSettingsForm {
  const query = useCommand("library_get_settings", { gameId });
  const [loadedFrom, setLoadedFrom] = useState<GameSettings | undefined>(undefined);
  const [saved, setSaved] = useState<GameSettings | null>(null);
  const [draft, setDraftState] = useState<GameSettings | null>(null);
  const [filesKey, setFilesKey] = useState<string | null>(null);
  const [savedFiles, setSavedFiles] = useState<FilesDraft | null>(null);
  const [files, setFilesState] = useState<FilesDraft | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<AppError | null>(null);

  // Sync from the backend whenever a new settings object arrives (load, save, reload).
  if (query.data && query.data !== loadedFrom) {
    setLoadedFrom(query.data);
    setSaved(query.data);
    setDraftState(query.data);
  }
  // Sync files only when they actually changed server-side, so status/notes
  // edits on the overview do not wipe unsaved path edits.
  const gameFilesKey = game ? JSON.stringify(filesOf(game)) : null;
  if (game && gameFilesKey !== filesKey) {
    setFilesKey(gameFilesKey);
    setSavedFiles(filesOf(game));
    setFilesState(filesOf(game));
  }

  const setDraft = useCallback((updater: (d: GameSettings) => GameSettings) => setDraftState((d) => (d ? updater(d) : d)), []);
  const setFiles = useCallback((updater: (f: FilesDraft) => FilesDraft) => setFilesState((f) => (f ? updater(f) : f)), []);

  const settingsDirty = !!draft && !!saved && JSON.stringify(draft) !== JSON.stringify(saved);
  const filesDirty = !!files && !!savedFiles && JSON.stringify(files) !== JSON.stringify(savedFiles);
  const dirty = settingsDirty || filesDirty;
  const errors = draft ? validateGameSettings(draft, files, t) : {};

  const save = async (): Promise<boolean> => {
    if (!draft || Object.keys(errors).length > 0) return false;
    setSaving(true);
    setSaveError(null);
    try {
      if (filesDirty && files && game) {
        const updated = await call("library_update_game", { game: { ...game, exePath: files.exePath.trim(), workingDir: files.workingDir?.trim() || null, launchArgs: files.launchArgs } });
        onGameSaved(updated);
      }
      if (settingsDirty) {
        const result = await call("library_save_settings", { gameId, settings: draft });
        query.mutate(result);
      }
      return true;
    } catch (e) {
      setSaveError(normalizeError(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const revert = () => {
    setDraftState(saved);
    setFilesState(savedFiles);
    setSaveError(null);
  };

  return {
    loading: query.loading,
    loadError: query.error,
    reload: query.reload,
    saved,
    draft,
    setDraft,
    files,
    setFiles,
    dirty,
    errors,
    saving,
    saveError,
    save,
    revert,
  };
}
