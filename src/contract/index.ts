// Typed IPC surface. `Commands` maps every Tauri command name to its argument
// object (keys are camelCase; Tauri converts them to snake_case Rust params)
// and its resolved result type. Rejections are `AppError`.
import type { GraphicsCommands } from "./graphics";
import type { LibraryCommands } from "./library";
import type { RuntimeCommands } from "./runtime";
import type { SessionCommands } from "./session";
import type { TranslationCommands } from "./translation";

export type * from "./core";
export type * from "./graphics";
export type * from "./library";
export type * from "./runtime";
export type * from "./session";
export type * from "./translation";

export type Commands = LibraryCommands & RuntimeCommands & GraphicsCommands & TranslationCommands & SessionCommands;
export type CommandName = keyof Commands;
export type CommandArgs<K extends CommandName> = Commands[K]["args"];
export type CommandResult<K extends CommandName> = Commands[K]["result"];
