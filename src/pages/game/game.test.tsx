import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import type { GameSettings } from "../../contract";
import { defaultTranslationConfig } from "../../lib/defaults";
import { getMockCalls, mockState, resetMockBackend } from "../../lib/mock";
import { renderApp } from "../../test/utils";

beforeEach(() => resetMockBackend());

describe("game settings form", () => {
  it("loads, edits, saves and round-trips settings", async () => {
    const user = userEvent.setup();
    renderApp("/game/2?tab=compatibility");
    const jp = await screen.findByRole("switch", { name: "Japanese locale" });
    expect(getMockCalls("library_get_settings")).toEqual([{ cmd: "library_get_settings", args: { gameId: 2 } }]);
    expect(jp).toHaveAttribute("aria-checked", "true");

    // Edit two things: locale off and a timezone.
    await user.click(jp);
    await user.type(screen.getByLabelText("Time zone (TZ)"), "Asia/Tokyo");
    expect(screen.getByRole("region", { name: "Unsaved changes" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(getMockCalls("library_save_settings")).toHaveLength(1));
    const saved = getMockCalls("library_save_settings")[0]!.args as { gameId: number; settings: GameSettings };
    expect(saved.gameId).toBe(2);
    expect(saved.settings.runtime.locale).toEqual({ japaneseLocale: false, locale: null, timezone: "Asia/Tokyo" });
    // untouched data is preserved
    expect(saved.settings.runtime.prefix).toEqual({ type: "shared", id: "pfx-shared-jp" });
    expect(saved.settings.runtime.auxiliary.map((a) => a.name)).toEqual(["Textractor"]);
    expect(mockState().settings.get(2)?.runtime.locale.timezone).toBe("Asia/Tokyo");

    // after the save the form is clean again
    await waitFor(() => expect(screen.queryByRole("region", { name: "Unsaved changes" })).not.toBeInTheDocument());
    expect(screen.getByRole("switch", { name: "Japanese locale" })).toHaveAttribute("aria-checked", "false");
  });

  it("reverts unsaved edits", async () => {
    const user = userEvent.setup();
    renderApp("/game/2?tab=compatibility");
    const jp = await screen.findByRole("switch", { name: "Japanese locale" });
    await user.click(jp);
    expect(jp).toHaveAttribute("aria-checked", "false");
    await user.click(screen.getByRole("button", { name: "Revert" }));
    expect(screen.getByRole("switch", { name: "Japanese locale" })).toHaveAttribute("aria-checked", "true");
    expect(getMockCalls("library_save_settings")).toHaveLength(0);
  });

  it("validates before saving (invalid env var name blocks Save)", async () => {
    const user = userEvent.setup();
    renderApp("/game/2?tab=compatibility");
    await screen.findByRole("switch", { name: "Japanese locale" });
    const env = screen.getAllByRole("group", { name: "Environment variables" })[0]!;
    await user.click(within(env).getByRole("button", { name: "Add" }));
    await user.type(within(env).getByLabelText("Environment variables: key 1"), "1BAD KEY");
    expect(screen.getByRole("button", { name: "Save changes" })).toBeDisabled();
    expect(screen.getByText(/1 problem/)).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: /Compatibility/ })).toHaveTextContent("1");
  });

  it("edits graphics with a live gamescope preview", async () => {
    const user = userEvent.setup();
    renderApp("/game/4?tab=graphics");
    await user.click(await screen.findByRole("switch", { name: "Use gamescope" }));
    await user.click(screen.getByRole("button", { name: "1280×720" }));
    expect(await screen.findByText(/gamescope -w 1280 -h 720/)).toBeInTheDocument();
    // Wine FSR is clearly separated from gamescope
    expect(screen.getByText("This is a different feature from gamescope's FSR filter")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(mockState().settings.get(4)?.graphics.gamescope).toMatchObject({ enabled: true, gameWidth: 1280, gameHeight: 720 }));
  });
});

describe("translation target language", () => {
  it("has no built-in default", () => {
    expect(defaultTranslationConfig().targetLanguage).toBeNull();
  });

  it("prompts to choose when neither the game nor the app define a target", async () => {
    mockState().app.translation.targetLanguage = null;
    renderApp("/game/1?tab=translation");
    const target = await screen.findByLabelText<HTMLSelectElement>("Target language");
    await waitFor(() => expect(target).not.toBeDisabled());
    expect(target.value).toBe("");
    expect(target.selectedOptions[0]?.textContent).toBe("Use app default (not set)");
    expect(screen.getAllByRole("alert").some((a) => a.textContent?.includes("Choose a target language"))).toBe(true);
    // English/Italian are offered as choices but never selected implicitly
    const options = [...target.options].map((o) => o.value);
    expect(options).toEqual(expect.arrayContaining(["en", "it", "de"]));
  });

  it("uses the app default when set, and saves an explicit choice", async () => {
    mockState().app.translation.targetLanguage = "de";
    const user = userEvent.setup();
    renderApp("/game/1?tab=translation");
    const target = await screen.findByLabelText<HTMLSelectElement>("Target language");
    await waitFor(() => expect(target.selectedOptions[0]?.textContent).toBe("Use app default (German)"));
    expect(screen.queryByText("Choose a target language")).not.toBeInTheDocument();

    await user.selectOptions(target, "fr");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(mockState().settings.get(1)?.translation.targetLanguage).toBe("fr"));

    // and back to "app default" = null
    await user.selectOptions(screen.getByLabelText("Target language"), "");
    await user.click(screen.getByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(mockState().settings.get(1)?.translation.targetLanguage).toBeNull());
  });

  it("app settings never pick a target language on their own", async () => {
    mockState().app.translation.targetLanguage = null;
    renderApp("/settings");
    const target = await screen.findByLabelText<HTMLSelectElement>("Default target language");
    await waitFor(() => expect(target).not.toBeDisabled());
    expect(target.value).toBe("");
    expect(target.selectedOptions[0]?.textContent).toBe("Not set — choose a language");
  });
});

describe("saves", () => {
  it("restores a backup only after explicit confirmation", async () => {
    const user = userEvent.setup();
    renderApp("/game/1?tab=saves");
    const restore = await screen.findByRole("button", { name: "Restore Before true route" });

    await user.click(restore);
    const dialog = screen.getByRole("alertdialog", { name: "Restore this backup?" });
    expect(within(dialog).getByText("A safety backup is made first")).toBeInTheDocument();
    expect(getMockCalls("saves_restore_backup")).toHaveLength(0);

    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    expect(getMockCalls("saves_restore_backup")).toHaveLength(0);

    await user.click(screen.getByRole("button", { name: "Restore Before true route" }));
    await user.click(within(screen.getByRole("alertdialog")).getByRole("button", { name: "Back up current saves and restore" }));
    await waitFor(() => expect(getMockCalls("saves_restore_backup")).toEqual([{ cmd: "saves_restore_backup", args: { backupId: 1, confirm: true } }]));
    expect(await screen.findByText("Restored 42 files")).toBeInTheDocument();
    // the safety backup shows up in the refreshed list
    expect(await screen.findByText("Safety backup before restoring #1")).toBeInTheDocument();
  });

  it("creates a labelled backup", async () => {
    const user = userEvent.setup();
    renderApp("/game/1?tab=saves");
    await user.type(await screen.findByLabelText("Label (optional)"), "Chapter 3");
    await user.click(screen.getByRole("button", { name: "Create backup" }));
    expect(await screen.findByText("Chapter 3")).toBeInTheDocument();
    expect(getMockCalls("saves_create_backup")[0]?.args).toEqual({ gameId: 1, label: "Chapter 3" });
  });
});

describe("game detail", () => {
  it("shows metadata, launches and stops a session", async () => {
    const user = userEvent.setup();
    renderApp("/game/1");
    expect(await screen.findByRole("heading", { level: 1, name: "Steins;Gate" })).toBeInTheDocument();
    expect(screen.getAllByText("シュタインズ・ゲート")[0]).toHaveAttribute("lang", "ja");
    await user.click(screen.getByRole("button", { name: "Play" }));
    expect(await screen.findByRole("button", { name: "Force stop" })).toBeInTheDocument();
    await waitFor(() => expect(getMockCalls("session_launch")).toHaveLength(1));
    await user.click(await screen.findByRole("button", { name: "Stop" }));
    expect(getMockCalls("session_stop")[0]?.args).toMatchObject({ force: false });
    await waitFor(() => expect(screen.getByRole("button", { name: "Play" })).toBeInTheDocument(), { timeout: 3000 });
  });

  it("shows the launch preview", async () => {
    renderApp("/game/1?tab=launch");
    expect(await screen.findByTestId("launch-preview")).toHaveTextContent("gamescope");
    expect(screen.getByTestId("launch-preview")).toHaveTextContent("umu-run");
  });
});
