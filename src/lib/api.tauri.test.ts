// Inside Tauri the real IPC must be used and the mock must never be touched.
import { beforeEach, describe, expect, it, vi } from "vitest";

type AnyFn = (...args: unknown[]) => unknown;
const invoke = vi.fn<AnyFn>();
const tauriListen = vi.fn<AnyFn>();

vi.mock("@tauri-apps/api/core", () => ({
  isTauri: () => true,
  invoke: (...args: unknown[]) => invoke(...args),
  convertFileSrc: (p: string) => `asset://localhost/${encodeURIComponent(p)}`,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (...args: unknown[]) => tauriListen(...args),
}));
const mockInvoke = vi.fn<AnyFn>();
vi.mock("./mock", () => ({ mockInvoke: (...a: unknown[]) => mockInvoke(...a), mockListen: vi.fn() }));

const { call, isDemoMode, listen } = await import("./api");
const { coverSrc } = await import("./cover");

beforeEach(() => {
  invoke.mockReset();
  tauriListen.mockReset();
  mockInvoke.mockReset();
});

describe("api inside Tauri", () => {
  it("uses invoke with camelCase args and never the mock", async () => {
    invoke.mockResolvedValue([]);
    expect(isDemoMode()).toBe(false);
    await call("library_play_sessions", { gameId: 3 });
    expect(invoke).toHaveBeenCalledWith("library_play_sessions", { gameId: 3 });
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("normalises invoke rejections", async () => {
    invoke.mockRejectedValue({ code: "missingDependency", message: "gamescope not found", details: "install it" });
    await expect(call("graphics_capabilities", {})).rejects.toEqual({ code: "missingDependency", message: "gamescope not found", details: "install it" });
    invoke.mockRejectedValue("Command foo not found");
    await expect(call("graphics_capabilities", {})).rejects.toMatchObject({ code: "notImplemented" });
  });

  it("unwraps event payloads", async () => {
    let handler: ((e: { payload: unknown }) => void) | undefined;
    tauriListen.mockImplementation((...args: unknown[]) => {
      handler = args[1] as (e: { payload: unknown }) => void;
      return Promise.resolve(() => undefined);
    });
    const got: unknown[] = [];
    await listen("vnl://task", (p) => got.push(p));
    handler?.({ payload: { taskId: "t", label: "x", state: "done", progress: 1, message: null } });
    expect(tauriListen).toHaveBeenCalledWith("vnl://task", expect.any(Function));
    expect(got).toEqual([{ taskId: "t", label: "x", state: "done", progress: 1, message: null }]);
  });

  it("converts cover paths with convertFileSrc", () => {
    expect(coverSrc({ coverPath: "/cache/c.jpg", coverUrl: null })).toBe("asset://localhost/%2Fcache%2Fc.jpg");
  });
});
