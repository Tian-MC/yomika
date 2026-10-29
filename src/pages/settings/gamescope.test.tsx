// Gamescope section of Settings: binaries, custom path and the optional Anime4K build.
import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { getMockCalls, mockState, resetMockBackend, setMockOverride } from "../../lib/mock";
import { renderApp } from "../../test/utils";

beforeEach(() => resetMockBackend());

describe("Settings > Gamescope", () => {
  it("lists the system gamescope and says Anime4K is unavailable, without building anything", async () => {
    renderApp("/settings");
    expect(await screen.findByText("Detected gamescope binaries")).toBeInTheDocument();
    expect(screen.getByText("/usr/bin/gamescope")).toBeInTheDocument();
    expect(screen.getByText(/Anime4K: not available/)).toBeInTheDocument();
    // The build is opt-in: nothing started on its own.
    expect(getMockCalls("gamescope_build_start")).toHaveLength(0);
    expect(screen.getByRole("button", { name: "Download and build" })).toBeEnabled();
  });

  it("builds on request, shows progress, then reports the installed build", async () => {
    const user = userEvent.setup();
    renderApp("/settings");
    await user.click(await screen.findByRole("button", { name: "Download and build" }));
    expect(getMockCalls("gamescope_build_start")).toHaveLength(1);
    expect(await screen.findByRole("button", { name: "Cancel build" })).toBeInTheDocument();
    expect(await screen.findByText("Anime4K build installed", {}, { timeout: 6000 })).toBeInTheDocument();
    expect(await screen.findByText(/Anime4K: available/)).toBeInTheDocument();
    expect(mockState().gamescope.managed).toBe(true);
  });

  it("can cancel a running build", async () => {
    const user = userEvent.setup();
    renderApp("/settings");
    await user.click(await screen.findByRole("button", { name: "Download and build" }));
    await user.click(await screen.findByRole("button", { name: "Cancel build" }));
    await waitFor(() => expect(getMockCalls("gamescope_build_cancel")).toHaveLength(1));
    expect((await screen.findAllByText("Build cancelled")).length).toBeGreaterThan(0);
    expect(mockState().gamescope.managed).toBe(false);
  });

  it("shows install hints instead of building when dependencies are missing", async () => {
    setMockOverride("gamescope_status", () => ({
      binaries: [],
      anime4kAvailable: false,
      managed: { installed: false, dir: "/data/tools/gamescope-anime4k", binary: null, manifest: null },
      requirements: {
        items: [],
        missing: ["meson", "libdecor-0"],
        hints: [{ distro: "arch", command: "sudo pacman -S --needed meson libdecor" }],
        ok: false,
      },
      build: { running: false, taskId: null, state: null, message: null, startedAt: null, finishedAt: null, logPath: null },
      repoUrl: "https://github.com/Et3rnos/gamescope-anime4k.git",
      branch: "master",
      customPath: null,
    }));
    renderApp("/settings");
    expect(await screen.findByText("Missing build dependencies (2)")).toBeInTheDocument();
    expect(screen.getByText("sudo pacman -S --needed meson libdecor")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download and build" })).toBeDisabled();
    expect(screen.getByText(/No gamescope found/)).toBeInTheDocument();
  });

  it("validates a custom path and keeps it in the settings draft", async () => {
    const user = userEvent.setup();
    renderApp("/settings");
    const field = await screen.findByLabelText("Custom gamescope binary");
    await user.type(field, "/opt/anime4k/bin/gamescope");
    expect(await screen.findByText(/Valid gamescope/)).toBeInTheDocument();
    expect(screen.getByText(/Supports Anime4K/)).toBeInTheDocument();
    await user.clear(field);
    await user.type(field, "relative/gamescope");
    expect(await screen.findByText(/full path/)).toBeInTheDocument();
    await user.clear(field);
    await user.type(field, "/opt/anime4k/bin/gamescope");
    await user.click(await screen.findByRole("button", { name: "Save changes" }));
    await waitFor(() => expect(mockState().app.gamescopePath).toBe("/opt/anime4k/bin/gamescope"));
  });
});
