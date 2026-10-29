// Entry point of the demo/mock backend. Loaded lazily by `lib/api.ts` only when
// the app is NOT running inside Tauri (browser dev server, Vitest).
import type { CommandArgs, CommandName, CommandResult } from "../../contract";
import { resetMockState, runHandler, seedStartupLogs } from "./backend";
import { clearMockListeners } from "./events";

export { mockEmit, mockListen } from "./events";
export { mockState } from "./backend";

export interface MockCall {
  cmd: CommandName;
  args: unknown;
}

const callLog: MockCall[] = [];
type Override = (args: unknown) => unknown;
const overrides = new Map<CommandName, Override>();

const LATENCY_MS = import.meta.env.MODE === "test" ? 0 : 90;

export async function mockInvoke<K extends CommandName>(cmd: K, args: CommandArgs<K>): Promise<CommandResult<K>> {
  const clonedArgs = structuredClone<CommandArgs<K>>(args);
  callLog.push({ cmd, args: clonedArgs });
  seedStartupLogs();
  await new Promise((r) => setTimeout(r, LATENCY_MS));
  const override = overrides.get(cmd);
  const result = override ? await override(clonedArgs) : await runHandler(cmd, clonedArgs);
  return structuredClone(result) as CommandResult<K>;
}

/** Calls recorded since the last reset (tests). */
export function getMockCalls(cmd?: CommandName): MockCall[] {
  return cmd ? callLog.filter((c) => c.cmd === cmd) : [...callLog];
}

/** Replace a command's behaviour (tests), e.g. to simulate `notImplemented`. */
export function setMockOverride<K extends CommandName>(cmd: K, fn: ((args: CommandArgs<K>) => CommandResult<K> | Promise<CommandResult<K>>) | null): void {
  if (fn) overrides.set(cmd, fn as Override);
  else overrides.delete(cmd);
}

export function resetMockBackend(): void {
  resetMockState();
  callLog.length = 0;
  overrides.clear();
  clearMockListeners();
}
