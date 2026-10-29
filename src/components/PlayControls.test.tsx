import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it } from "vitest";
import { Providers } from "../App";
import type { SessionInfo } from "../contract";
import { getMockCalls, mockEmit, mockState, resetMockBackend, setMockOverride } from "../lib/mock";
import { useSessions } from "../state/app";
import { PlayControls } from "./PlayControls";

function snapshot(id: string, state: SessionInfo["state"]): SessionInfo {
  return {
    id, gameId: 1, state, startedAt: 100, endedAt: state === "exited" || state === "failed" ? 101 : null,
    pid: 123, exitCode: null, error: null, runnerId: null, prefixPath: null, aux: [], commandPreview: "game.exe",
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => { resolve = done; });
  return { promise, resolve };
}

function SessionProbe() {
  const { sessions } = useSessions();
  return <output data-testid="sessions">{sessions.map((s) => `${s.id}:${s.state}`).join(",")}</output>;
}

function renderControls() {
  return render(<Providers><PlayControls game={mockState().games[0]!} /><SessionProbe /></Providers>);
}

beforeEach(() => resetMockBackend());

describe("session races", () => {
  it.each(["exited", "failed"] as const)("keeps %s when termination arrives before the launch response", async (terminal) => {
    const launch = deferred<SessionInfo>();
    const entered = deferred<void>();
    setMockOverride("session_launch", () => { entered.resolve(); return launch.promise; });
    const user = userEvent.setup();
    renderControls();
    await user.click(screen.getByRole("button", { name: "Play" }));
    await entered.promise;
    act(() => mockEmit("vnl://session", snapshot("first", terminal)));
    expect(screen.getByTestId("sessions")).toHaveTextContent(`first:${terminal}`);
    await act(async () => { launch.resolve(snapshot("first", "running")); await launch.promise; });
    await waitFor(() => expect(screen.getByRole("button", { name: "Play" })).toBeEnabled());
    expect(screen.queryByRole("button", { name: "Stop" })).not.toBeInTheDocument();
    expect(screen.getByTestId("sessions")).toHaveTextContent(`first:${terminal}`);

    // A fresh ID for the same game must be independent of the terminal one.
    setMockOverride("session_launch", () => snapshot("second", "running"));
    await user.click(screen.getByRole("button", { name: "Play" }));
    expect(await screen.findByRole("button", { name: "Stop" })).toBeInTheDocument();
    act(() => mockEmit("vnl://session", snapshot("first", "starting")));
    setMockOverride("session_stop", ({ sessionId }) => {
      mockEmit("vnl://session", snapshot(sessionId, "exited"));
      return null;
    });
    await user.click(screen.getByRole("button", { name: "Stop" }));
    await waitFor(() => expect(screen.getByRole("button", { name: "Play" })).toBeEnabled());
    expect(getMockCalls("session_stop")[0]?.args).toEqual({ sessionId: "second", force: false });
    expect(screen.getByTestId("sessions")).toHaveTextContent(`first:${terminal}`);
    expect(screen.getByTestId("sessions")).toHaveTextContent("second:exited");
  });

  it("does not regress stopping on a late launch response or older events", async () => {
    const launch = deferred<SessionInfo>();
    const entered = deferred<void>();
    setMockOverride("session_launch", () => { entered.resolve(); return launch.promise; });
    const user = userEvent.setup();
    renderControls();
    await user.click(screen.getByRole("button", { name: "Play" }));
    await entered.promise;
    act(() => mockEmit("vnl://session", snapshot("first", "starting")));
    setMockOverride("session_stop", ({ sessionId }) => {
      mockEmit("vnl://session", snapshot(sessionId, "stopping"));
      return null;
    });
    await user.click(screen.getByRole("button", { name: "Starting…" }));
    await waitFor(() => expect(screen.getByTestId("sessions")).toHaveTextContent("first:stopping"));
    await act(async () => { launch.resolve(snapshot("first", "running")); await launch.promise; });
    act(() => mockEmit("vnl://session", snapshot("first", "starting")));
    expect(screen.getByTestId("sessions")).toHaveTextContent("first:stopping");
    act(() => mockEmit("vnl://session", snapshot("first", "exited")));
    expect(await screen.findByRole("button", { name: "Play" })).toBeEnabled();
  });

  it("merges the delayed initial session list without reviving a finished session", async () => {
    const list = deferred<SessionInfo[]>();
    const entered = deferred<void>();
    setMockOverride("session_list", () => { entered.resolve(); return list.promise; });
    renderControls();
    await entered.promise;
    act(() => mockEmit("vnl://session", snapshot("first", "exited")));
    await act(async () => { list.resolve([snapshot("first", "running"), snapshot("other", "running")]); await list.promise; });
    await waitFor(() => expect(screen.getByTestId("sessions")).toHaveTextContent("other:running"));
    expect(screen.getByTestId("sessions")).toHaveTextContent("first:exited");
  });
});
