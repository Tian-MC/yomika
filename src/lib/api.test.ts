import { rejectWith } from "../test/reject";
import { beforeEach, describe, expect, it } from "vitest";
import { call, isDemoMode, isTauri, listen, normalizeError } from "./api";
import { getMockCalls, mockEmit, resetMockBackend, setMockOverride } from "./mock";

beforeEach(() => resetMockBackend());

describe("api routing (outside Tauri)", () => {
  it("detects that it is not running inside Tauri", () => {
    expect(isTauri()).toBe(false);
    expect(isDemoMode()).toBe(true);
  });

  it("routes commands to the mock backend with typed results", async () => {
    const games = await call("library_list_games", { query: { sort: "title" } });
    expect(games.length).toBeGreaterThan(3);
    expect(games.map((g) => g.title)).toEqual([...games.map((g) => g.title)].sort((a, b) => a.localeCompare(b)));
    expect(getMockCalls("library_list_games")).toHaveLength(1);
    expect(getMockCalls("library_list_games")[0]?.args).toEqual({ query: { sort: "title" } });
  });

  it("returns copies so callers cannot mutate backend state", async () => {
    const g = await call("library_get_game", { id: 1 });
    g.title = "mutated";
    expect((await call("library_get_game", { id: 1 })).title).not.toBe("mutated");
  });

  it("rejects with a normalised AppError from the mock", async () => {
    await expect(call("library_get_game", { id: 9999 })).rejects.toMatchObject({ code: "notFound", details: null });
  });

  it("normalises raw string rejections (e.g. notImplemented commands)", async () => {
    setMockOverride("diagnostics_system", () => rejectWith("command diagnostics_system not found"));
    await expect(call("diagnostics_system", {})).rejects.toEqual({ code: "notImplemented", message: "command diagnostics_system not found", details: null });
  });

  it("passes AppError rejections through, keeping notImplemented", async () => {
    setMockOverride("translation_providers", () => rejectWith({ code: "notImplemented", message: "later", details: null }));
    await expect(call("translation_providers", {})).rejects.toEqual({ code: "notImplemented", message: "later", details: null });
  });

  it("enforces confirmation for save restores", async () => {
    await expect(call("saves_restore_backup", { backupId: 1, confirm: false })).rejects.toMatchObject({ code: "confirmationRequired" });
    const res = await call("saves_restore_backup", { backupId: 1, confirm: true });
    expect(res.safetyBackup?.kind).toBe("safety");
  });
});

describe("normalizeError", () => {
  it("keeps a valid AppError", () => {
    expect(normalizeError({ code: "io", message: "disk", details: "x" })).toEqual({ code: "io", message: "disk", details: "x" });
  });
  it("maps unknown codes to internal and stringifies object details", () => {
    expect(normalizeError({ code: "weird", message: "m", details: { a: 1 } })).toEqual({ code: "internal", message: "m", details: '{"a":1}' });
  });
  it("handles missing details", () => {
    expect(normalizeError({ code: "network", message: "m" })).toEqual({ code: "network", message: "m", details: null });
  });
  it("handles Error instances", () => {
    const e = normalizeError(new Error("boom"));
    expect(e.code).toBe("internal");
    expect(e.message).toBe("boom");
  });
  it("handles plain strings and junk", () => {
    expect(normalizeError("oops")).toEqual({ code: "internal", message: "oops", details: null });
    expect(normalizeError(undefined)).toMatchObject({ code: "internal", details: null });
    expect(normalizeError(42)).toMatchObject({ code: "internal", details: "42" });
    expect(normalizeError({ foo: 1 })).toMatchObject({ code: "internal", details: '{"foo":1}' });
  });
});

describe("events (outside Tauri)", () => {
  it("delivers mock events to listeners until unlistened", async () => {
    const seen: string[] = [];
    const unlisten = await listen("vnl://log", (e) => seen.push(e.message));
    mockEmit("vnl://log", { tsMs: 1, level: "info", source: "test", sessionId: null, message: "hello" });
    unlisten();
    mockEmit("vnl://log", { tsMs: 2, level: "info", source: "test", sessionId: null, message: "ignored" });
    expect(seen).toEqual(["hello"]);
  });

  it("emits session events when launching through the mock", async () => {
    const states: string[] = [];
    const unlisten = await listen("vnl://session", (s) => states.push(s.state));
    const s = await call("session_launch", { gameId: 2 });
    expect(s.state).toBe("starting");
    expect(states).toContain("starting");
    unlisten();
  });
});
