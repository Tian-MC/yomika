// Library / database / VNDB / saves contract. Owned by Agent 2 (library).
// Rust side: crates/vnl-library + src-tauri/src/commands/library.rs
import type { AppSettings, Game, GameId, GameQuery, GameSettings } from "./core";

export interface ExeCandidate {
  path: string;
  relativePath: string;
  sizeBytes: number;
  /** Higher = more likely the main game executable. */
  score: number;
  /** Short machine-friendly hints, e.g. ["largest", "matchesFolderName"]. */
  hints: string[];
  isInstaller: boolean;
}

export interface ImportScan {
  root: string;
  /** Sorted by score, best first. */
  candidates: ExeCandidate[];
  suggestedTitle: string;
  /** The scanned path itself looks like an installer (setup.exe, *.msi...). */
  isInstaller: boolean;
  /** Set when a game with this exe is already in the library. */
  existingGameId: GameId | null;
}

export interface ImportRequest {
  title: string;
  exePath: string;
  installDir?: string | null;
  workingDir?: string | null;
  launchArgs?: string[];
  vndbId?: string | null;
  /** Defaults: AppSettings-derived when omitted. */
  settings?: GameSettings | null;
}

export interface PlaySession {
  id: number;
  gameId: GameId;
  startedAt: number;
  endedAt: number;
  durationSeconds: number;
}

export interface VndbSearchResult {
  id: string;
  title: string;
  altTitle: string | null;
  released: string | null;
  developers: string[];
  imageUrl: string | null;
  imageSexual: number | null;
  rating: number | null;
}

export type SaveLocationSource = "detected" | "manual";

export interface SaveLocation {
  path: string;
  source: SaveLocationSource;
  exists: boolean;
  /** Why this location was suggested, e.g. "AppData/Roaming match". */
  reason: string | null;
}

export interface SaveBackup {
  id: number;
  gameId: GameId;
  createdAt: number;
  label: string | null;
  sizeBytes: number;
  archivePath: string;
  sourcePaths: string[];
}

export interface RestoreResult {
  restored: SaveBackup;
  /** Backup of the current saves taken automatically before restoring. */
  safetyBackup: SaveBackup | null;
}

export interface LibraryCommands {
  library_list_games: { args: { query: GameQuery }; result: Game[] };
  library_get_game: { args: { id: GameId }; result: Game };
  /** Inspect an .exe or a folder before importing. */
  library_scan_path: { args: { path: string }; result: ImportScan };
  library_import_game: { args: { request: ImportRequest }; result: Game };
  /** Replaces user-editable fields (title, paths, args, status, favorite, metadata, notes). */
  library_update_game: { args: { game: Game }; result: Game };
  /** Removes the DB entry only — never touches game files or prefixes. */
  library_remove_game: { args: { id: GameId }; result: null };
  library_get_settings: { args: { gameId: GameId }; result: GameSettings };
  library_save_settings: { args: { gameId: GameId; settings: GameSettings }; result: GameSettings };
  library_play_sessions: { args: { gameId: GameId }; result: PlaySession[] };

  settings_get_app: { args: Record<string, never>; result: AppSettings };
  settings_save_app: { args: { settings: AppSettings }; result: AppSettings };

  vndb_search: { args: { query: string }; result: VndbSearchResult[] };
  /** Fetch full metadata + cover for vndbId and store it on the game. */
  vndb_apply: { args: { gameId: GameId; vndbId: string }; result: Game };

  saves_detect: { args: { gameId: GameId }; result: SaveLocation[] };
  saves_set_locations: { args: { gameId: GameId; paths: string[] }; result: SaveLocation[] };
  saves_list_backups: { args: { gameId: GameId }; result: SaveBackup[] };
  saves_create_backup: { args: { gameId: GameId; label?: string | null }; result: SaveBackup };
  /** Fails with code "confirmationRequired" unless confirm === true. */
  saves_restore_backup: { args: { backupId: number; confirm: boolean }; result: RestoreResult };
}
