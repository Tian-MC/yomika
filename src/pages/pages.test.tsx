// Smoke + behaviour tests for the remaining screens.
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { rejectWith } from "../test/reject";
import { beforeEach, describe, expect, it } from "vitest";
import { getMockCalls, mockEmit, mockState, resetMockBackend, setMockOverride } from "../lib/mock";
import { renderApp } from "../test/utils";
import { resolveUiLanguage } from "../i18n";

beforeEach(() => resetMockBackend());

describe("Runners & Prefixes", () => {
  it("lists runners and prefixes and installs a missing component via winetricks", async () => {
    const user = userEvent.setup();
    renderApp("/runtime");
    expect(await screen.findByText("GE-Proton9-20")).toBeInTheDocument();
    const card = await screen.findByRole("article", { name: "Shared JP prefix" });
    await user.click(within(card).getByRole("button", { name: "Check fonts & codecs" }));
    const install = await screen.findAllByRole("button", { name: "Install via winetricks" });
    await user.click(install[0]!);
    await waitFor(() => expect(getMockCalls("runtime_prefix_tool")).toHaveLength(1));
    expect(getMockCalls("runtime_prefix_tool")[0]?.args).toEqual({ request: { prefixId: "pfx-shared-jp", runnerId: "system-wine", tool: "winetricks", verbs: ["fakejapanese"] } });
  });
});

describe("Translation page", () => {
  it("translates manually and shows the provider", async () => {
    mockState().app.translation.targetLanguage = "en";
    const user = userEvent.setup();
    renderApp("/translation");
    await user.type(await screen.findByLabelText("Text"), "桜の花びらが、静かに舞い落ちていく。");
    await user.click(screen.getByRole("button", { name: "Translate" }));
    expect(await screen.findByText("Cherry petals drift down quietly.")).toBeInTheDocument();
    expect(screen.getByText(/Translated by Argos Translate/)).toBeInTheDocument();
  });

  it("asks for a target language instead of assuming one", async () => {
    mockState().app.translation.targetLanguage = null;
    renderApp("/translation");
    expect(await screen.findByText("Choose a target language")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Translate" })).toBeDisabled();
  });

  it("flags remote providers and keeps / deletes API keys explicitly", async () => {
    const user = userEvent.setup();
    renderApp("/translation?tab=providers");
    const card = await screen.findByRole("article", { name: "OpenAI-compatible API" });
    expect(within(card).getByText("Sends text online")).toBeInTheDocument();
    expect(within(card).getByText("Dialogue will leave this computer")).toBeInTheDocument();
    // Save without touching the key => apiKey omitted (keep)
    await user.clear(within(card).getByLabelText("Model *"));
    await user.type(within(card).getByLabelText("Model *"), "big");
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(getMockCalls("translation_configure_provider")).toHaveLength(1));
    expect(getMockCalls("translation_configure_provider")[0]?.args).not.toHaveProperty("apiKey");
    // Delete key => apiKey: "" (contract: empty string deletes)
    await user.click(within(card).getByRole("button", { name: "Delete key" }));
    await user.click(within(card).getByRole("button", { name: "Save" }));
    await waitFor(() => expect(getMockCalls("translation_configure_provider")).toHaveLength(2));
    expect(getMockCalls("translation_configure_provider")[1]?.args).toMatchObject({ providerId: "openai-compatible", apiKey: "" });
  });

  it("edits the glossary", async () => {
    const user = userEvent.setup();
    renderApp("/translation?tab=glossary");
    expect(await screen.findByText("先輩")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add name" }));
    await user.type(screen.getByRole("textbox", { name: "Original" }), "椎名まゆり");
    await user.type(screen.getByRole("textbox", { name: "Translation" }), "Mayuri Shiina");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("Mayuri Shiina")).toBeInTheDocument();
    expect(getMockCalls("translation_glossary_upsert")[0]?.args).toMatchObject({ entry: { id: 0, gameId: null, kind: "characterName", source: "椎名まゆり" } });
  });
});

describe("Diagnostics", () => {
  it("groups dependencies and streams logs", async () => {
    renderApp("/diagnostics");
    expect(await screen.findByText("umu-launcher")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Runtime" })).toBeInTheDocument();
    const log = await screen.findByRole("log");
    act(() => mockEmit("vnl://log", { tsMs: Date.now(), level: "warn", source: "test", sessionId: null, message: "live log line" }));
    expect(await within(log).findByText("live log line")).toBeInTheDocument();
  });
});

describe("Overlay", () => {
  it("renders chrome-less and shows incoming lines", async () => {
    renderApp("/overlay");
    expect(await screen.findByText("Waiting for text…")).toBeInTheDocument();
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
    expect(document.documentElement.classList.contains("overlay-mode")).toBe(true);
    act(() =>
      mockEmit("vnl://translation", { id: "l1", sessionId: null, gameId: null, origin: "hook", sourceText: "「またね」", translatedText: "See you.", sourceLanguage: "ja", targetLanguage: "en", providerId: null, error: null, tsMs: 1 }),
    );
    expect(await screen.findByText("See you.")).toBeInTheDocument();
    expect(screen.getByText("「またね」")).toHaveAttribute("lang", "ja");
    // live config pushed by overlay_show
    act(() => mockEmit("vnl://overlay-config", { enabled: true, fontSize: 30, opacity: 0.5, showOriginal: false }));
    await waitFor(() => expect(screen.queryByText("「またね」")).not.toBeInTheDocument());
    expect(screen.getByText("See you.").closest(".overlay-panel")).toHaveStyle({ fontSize: "30px" });
  });
});

describe("Shell", () => {
  it("shows running sessions and background tasks from events", async () => {
    renderApp("/settings");
    await screen.findByRole("heading", { name: "Settings" });
    act(() => mockEmit("vnl://task", { taskId: "t1", label: "winetricks cjkfonts", state: "running", progress: 0.4, message: "Downloading" }));
    expect(await screen.findByText("winetricks cjkfonts")).toBeInTheDocument();
    act(() =>
      mockEmit("vnl://session", { id: "s1", gameId: 1, state: "running", startedAt: Math.floor(Date.now() / 1000), endedAt: null, pid: 1, exitCode: null, error: null, runnerId: null, prefixPath: null, aux: [], commandPreview: "" }),
    );
    expect(await screen.findByRole("status", { name: "1 game(s) running" })).toHaveTextContent("Steins;Gate");
  });

  it("surfaces notImplemented errors as friendly toasts", async () => {
    setMockOverride("session_launch", () => rejectWith({ code: "notImplemented", message: "session_launch is not implemented yet", details: null }));
    const user = userEvent.setup();
    renderApp("/game/1");
    await user.click(await screen.findByRole("button", { name: "Play" }));
    expect(await screen.findByText("Could not launch Steins;Gate")).toBeInTheDocument();
    expect(screen.getByText("This part of the backend is not implemented yet.")).toBeInTheDocument();
  });

  it("switches the UI language from settings", async () => {
    expect(resolveUiLanguage(null, "it-IT")).toBe("it");
    expect(resolveUiLanguage("en", "it-IT")).toBe("en");
    expect(resolveUiLanguage(null, "ja-JP")).toBe("en");
    const user = userEvent.setup();
    renderApp("/settings");
    await user.selectOptions(await screen.findByLabelText("Interface language"), "it");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    expect(await screen.findByRole("heading", { name: "Impostazioni" })).toBeInTheDocument();
    expect(mockState().app.uiLanguage).toBe("it");
  });
});
