import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ImportRequest } from "../../contract";
import { getMockCalls, mockState, resetMockBackend } from "../../lib/mock";
import { renderApp } from "../../test/utils";

beforeEach(() => resetMockBackend());

describe("import wizard", () => {
  it("happy path: folder → candidate → title + VNDB → runner → import → apply metadata", async () => {
    const user = userEvent.setup();
    const { router } = renderApp("/import");

    await user.type(await screen.findByLabelText("…or type a path"), "/home/demo/Games/VN/Sakura");
    await user.click(screen.getByRole("button", { name: "Scan" }));

    // candidates, best first, with hints
    const group = await screen.findByRole("radiogroup", { name: "Executable candidates" });
    const radios = within(group).getAllByRole("radio");
    expect(radios[0]).toBeChecked();
    expect(within(group).getByText("Best match")).toBeInTheDocument();
    expect(within(group).getByText("Matches folder name")).toBeInTheDocument();
    expect(getMockCalls("library_scan_path")[0]?.args).toEqual({ path: "/home/demo/Games/VN/Sakura" });
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // title prefilled from scan, VNDB search & select
    const title = screen.getByLabelText("Title");
    expect(title).toHaveValue("Sakura");
    await user.clear(screen.getByRole("searchbox", { name: "Search VNDB" }));
    await user.type(screen.getByRole("searchbox", { name: "Search VNDB" }), "Sakura no Uta");
    await user.click(screen.getByRole("button", { name: "Search" }));
    await user.click(await screen.findByRole("button", { name: /Sakura no Uta/ }));
    await user.click(screen.getByRole("button", { name: "Use VNDB title" }));
    expect(screen.getByLabelText("Title")).toHaveValue("Sakura no Uta");
    await user.click(screen.getByRole("button", { name: "Continue" }));

    // runner step: pick Proton explicitly
    await user.selectOptions(screen.getByLabelText("Runner"), "proton-ge-9-20");
    await user.click(screen.getByRole("button", { name: "Add to library" }));

    await waitFor(() => expect(router.state.location.pathname).toMatch(/^\/game\/\d+$/));
    const req = getMockCalls("library_import_game")[0]?.args as { request: ImportRequest };
    expect(req.request).toMatchObject({ title: "Sakura no Uta", exePath: "/home/demo/Games/VN/Sakura/Sakura.exe", installDir: "/home/demo/Games/VN/Sakura", vndbId: "v5154" });
    expect(req.request.settings?.runtime.runner).toEqual({ id: "proton-ge-9-20" });
    expect(req.request.settings?.translation.targetLanguage).toBeNull();
    expect(getMockCalls("vndb_apply")[0]?.args).toMatchObject({ vndbId: "v5154" });
    expect(await screen.findByRole("heading", { level: 1, name: "Sakura no Uta" })).toBeInTheDocument();
    expect(mockState().games.some((g) => g.title === "Sakura no Uta" && g.metadata.vndbId === "v5154")).toBe(true);
  });

  it("uses the native picker (demo prompt) and sends default settings as null", async () => {
    const prompt = vi.spyOn(window, "prompt").mockReturnValue("/home/demo/Games/VN/Kanon/kanon.exe");
    const user = userEvent.setup();
    renderApp("/import");
    await user.click(await screen.findByRole("button", { name: /Choose \.exe/ }));
    expect(prompt).toHaveBeenCalled();
    await screen.findByRole("radiogroup", { name: "Executable candidates" });
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await user.click(screen.getByRole("button", { name: "Add to library" }));
    await waitFor(() => expect(getMockCalls("library_import_game")).toHaveLength(1));
    const req = getMockCalls("library_import_game")[0]!.args as { request: ImportRequest };
    expect(req.request.exePath).toBe("/home/demo/Games/VN/Kanon/kanon.exe");
    expect(req.request.settings).toBeNull();
    expect(getMockCalls("vndb_apply")).toHaveLength(0);
    prompt.mockRestore();
  });

  it("installer path: runs the installer, tracks progress, then scans drive_c", async () => {
    const user = userEvent.setup();
    renderApp("/import");
    await user.type(await screen.findByLabelText("…or type a path"), "/home/demo/Downloads/game/setup.exe");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    expect(await screen.findByText("This looks like an installer")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Run installer" }));
    await waitFor(() => expect(getMockCalls("runtime_run_installer")).toHaveLength(1));
    expect(getMockCalls("runtime_run_installer")[0]?.args).toMatchObject({ request: { installerPath: "/home/demo/Downloads/game/setup.exe", runnerId: "system-wine", prefix: { type: "dedicated" } } });
    // progress then automatic scan of the prefix's drive_c
    await screen.findByRole("radiogroup", { name: "Executable candidates" }, { timeout: 5000 });
    const scans = getMockCalls("library_scan_path").map((c) => (c.args as { path: string }).path);
    expect(scans.at(-1)).toMatch(/\/drive_c$/);
  });

  it("shows scan errors inline", async () => {
    const user = userEvent.setup();
    renderApp("/import");
    await user.type(await screen.findByLabelText("…or type a path"), "relative/path");
    await user.click(screen.getByRole("button", { name: "Scan" }));
    expect(await screen.findByText("Path must be absolute")).toBeInTheDocument();
  });
});
