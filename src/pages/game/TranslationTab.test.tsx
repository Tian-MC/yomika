import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, expect, it } from "vitest";
import type { HookThread, SessionInfo, TranslationLine, TranslationSessionStatus } from "../../contract";
import { getMockCalls, mockEmit, mockState, resetMockBackend, setMockOverride } from "../../lib/mock";
import { renderApp } from "../../test/utils";

const thread: HookThread = {
  id: "164:17", selector: { name: "System43", hookCode: "HS4@0:Game.exe", context: "0", context2: "19F95C" },
  preview: "最初の台詞", received: 1,
};
let status: TranslationSessionStatus;

beforeEach(() => {
  resetMockBackend();
  const session: SessionInfo = {
    id: "hook-session", gameId: 1, state: "running", startedAt: 100, endedAt: null,
    pid: 123, exitCode: null, error: null, runnerId: null, prefixPath: null, aux: [], commandPreview: "game.exe",
  };
  mockState().sessions.set(session.id, session);
  mockState().app.translation.providerId = null;
  mockState().settings.get(1)!.translation.providerId = null;
  status = { sessionId: session.id, hook: "active", ocr: "inactive", hookThreads: [structuredClone(thread)], selectedHookThread: null, diagnostics: [] };
  setMockOverride("translation_session_status", () => status);
  setMockOverride("translation_hook_select_thread", ({ sessionId, gameId, threadId }) => {
    if (sessionId !== "hook-session" || gameId !== 1) throw new Error("Wrong session");
    const selected = threadId ? status.hookThreads.find((t) => t.id === threadId)!.selector : null;
    status.selectedHookThread = threadId;
    mockState().settings.get(gameId)!.translation.hook.thread = selected;
    return selected;
  });
});

it("selects, pauses and persists the thread without losing other unsaved settings", async () => {
  const user = userEvent.setup();
  const view = renderApp("/game/1?tab=translation");
  const selector = await screen.findByLabelText("Text thread");
  expect(await screen.findByText(thread.preview)).toBeInTheDocument();
  const delay = screen.getByLabelText("Attach delay");
  await user.clear(delay);
  await user.type(delay, "500");
  await user.selectOptions(selector, thread.id);
  await waitFor(() => expect(getMockCalls("translation_hook_select_thread")[0]?.args).toEqual({ sessionId: "hook-session", gameId: 1, threadId: thread.id }));
  await waitFor(() => expect(selector).toHaveValue(thread.id));
  expect(mockState().settings.get(1)!.translation.hook.thread).toEqual(thread.selector);
  expect(mockState().settings.get(2)!.translation.hook.thread).toBeNull();
  expect(delay).toHaveValue(500);
  expect(screen.getByRole("region", { name: "Unsaved changes" })).toBeInTheDocument();
  await user.click(screen.getByRole("button", { name: "Save changes" }));
  await waitFor(() => expect(mockState().settings.get(1)!.translation.hook.attachDelayMs).toBe(500));
  expect(mockState().settings.get(1)!.translation.hook.thread).toEqual(thread.selector);
  view.unmount();
  renderApp("/game/1?tab=translation");
  expect(await screen.findByLabelText("Text thread")).toHaveValue(thread.id);
  await user.selectOptions(screen.getByLabelText("Text thread"), "");
  await waitFor(() => expect(mockState().settings.get(1)!.translation.hook.thread).toBeNull());
});

it("polls updated previews and displays original text without a provider for this session only", async () => {
  renderApp("/game/1?tab=translation");
  await screen.findByLabelText("Text thread");
  status.hookThreads[0]!.preview = "更新された台詞";
  await screen.findByText("更新された台詞", {}, { timeout: 2500 });
  const line: TranslationLine = {
    id: "line", sessionId: "another-session", gameId: 1, origin: "hook", sourceText: "foreign session",
    translatedText: null, sourceLanguage: "ja", targetLanguage: null, providerId: null, error: null, tsMs: 1,
  };
  act(() => mockEmit("vnl://translation", line));
  expect(screen.queryByText("foreign session")).not.toBeInTheDocument();
  act(() => mockEmit("vnl://translation", { ...line, sessionId: "hook-session", sourceText: "抽出した原文" }));
  expect(await screen.findByText("抽出した原文")).toBeInTheDocument();
  act(() => mockEmit("vnl://translation", { ...line, id: "newer", sessionId: "hook-session", sourceText: "次の台詞", tsMs: 2 }));
  act(() => mockEmit("vnl://translation", { ...line, sessionId: "hook-session", sourceText: "抽出した原文", translatedText: "late response" }));
  expect(screen.getByText("次の台詞")).toBeInTheDocument();
  expect(screen.queryByText("late response")).not.toBeInTheDocument();
});

it("reports selection failure and preserves the saved choice", async () => {
  setMockOverride("translation_hook_select_thread", () => { throw Object.assign(new Error("Game session ended"), { code: "notFound" }); });
  const user = userEvent.setup();
  renderApp("/game/1?tab=translation");
  await user.selectOptions(await screen.findByLabelText("Text thread"), thread.id);
  expect(await screen.findByText("Game session ended")).toBeInTheDocument();
  expect(mockState().settings.get(1)!.translation.hook.thread).toBeNull();
  expect(screen.getByLabelText("Text thread")).toHaveValue("");
});
