import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { rejectWith } from "../../test/reject";
import { beforeEach, describe, expect, it } from "vitest";
import type { Game } from "../../contract";
import { resetMockBackend, mockState } from "../../lib/mock";
import { seedGames } from "../../lib/mock/data";
import { renderApp } from "../../test/utils";
import { DEFAULT_FILTER, filterAndSortGames, tagCounts } from "./filter";

const NOW = 1_750_000_000;
const games: Game[] = seedGames(NOW);
const titles = (list: Game[]) => list.map((g) => g.title);

describe("filterAndSortGames", () => {
  it("sorts by last played (desc) with never-played games last", () => {
    const out = filterAndSortGames(games, DEFAULT_FILTER);
    expect(out[0]?.title).toBe("Clannad");
    const tail = out.slice(-2).map((g) => g.lastPlayedAt);
    expect(tail).toEqual([null, null]);
    // nulls stay last in ascending order too
    const asc = filterAndSortGames(games, { ...DEFAULT_FILTER, descending: false });
    expect(asc.slice(-2).every((g) => g.lastPlayedAt === null)).toBe(true);
    expect(asc[0]?.title).toBe("Ever17 -the out of infinity-");
  });

  it("supports every sort key in both directions", () => {
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "title", descending: false }))[0]).toBe("Clannad");
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "title", descending: true }))[0]).toBe("Untitled doujin VN");
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "playtime", descending: true }))[0]).toBe("Steins;Gate");
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "addedAt", descending: true }))[0]).toBe("Untitled doujin VN");
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "releaseDate", descending: false }))[0]).toBe("Higurashi no Naku Koro ni");
    // game without a release date goes last
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, sort: "releaseDate", descending: true })).at(-1)).toBe("Untitled doujin VN");
  });

  it("filters by status, favorites, tag and search (incl. Japanese titles)", () => {
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, status: "playing", sort: "title", descending: false }))).toEqual(["Clannad", "Higurashi no Naku Koro ni"]);
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, favoriteOnly: true, sort: "title", descending: false }))).toEqual(["Clannad", "Steins;Gate"]);
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, tag: "Horror", sort: "title", descending: false }))).toEqual(["Higurashi no Naku Koro ni", "Saya no Uta"]);
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, search: "沙耶" }))).toEqual(["Saya no Uta"]);
    expect(titles(filterAndSortGames(games, { ...DEFAULT_FILTER, search: "nitroplus" })).sort()).toEqual(["Saya no Uta", "Steins;Gate"]);
    expect(filterAndSortGames(games, { ...DEFAULT_FILTER, search: "zzz" })).toEqual([]);
  });

  it("stays fast with hundreds of games", () => {
    const many = Array.from({ length: 1500 }, (_, i) => ({ ...games[i % games.length]!, id: i + 1, title: `Game ${i}` }));
    const t0 = performance.now();
    const out = filterAndSortGames(many, { ...DEFAULT_FILTER, search: "game 1", sort: "title", descending: false });
    expect(performance.now() - t0).toBeLessThan(200);
    expect(out[0]?.title).toBe("Game 1");
    expect(tagCounts(many)[0]?.[1]).toBeGreaterThan(100);
  });
});

describe("Library page", () => {
  beforeEach(() => {
    resetMockBackend();
    try {
      localStorage.clear();
    } catch {
      /* ignore */
    }
  });

  it("shows the demo badge and all games as cover cards", async () => {
    renderApp("/");
    expect(await screen.findAllByTestId("game-card")).toHaveLength(7);
    expect(screen.getByRole("note")).toHaveTextContent(/Demo mode/);
    // Japanese original titles are tagged for the JP font stack
    expect(screen.getByText("沙耶の唄")).toHaveAttribute("lang", "ja");
  });

  it("filters by status and favorites, and sorts by title", async () => {
    const user = userEvent.setup();
    renderApp("/");
    await screen.findAllByTestId("game-card");

    await user.selectOptions(screen.getByLabelText("Status"), "completed");
    expect(screen.getAllByTestId("game-card")).toHaveLength(1);
    expect(screen.getByText("7 games", { exact: false }).textContent).toMatch(/1 of 7/);

    await user.selectOptions(screen.getByLabelText("Status"), "");
    await user.click(screen.getByRole("button", { name: /Favorites/ }));
    const favs = screen.getAllByTestId("game-card").map((c) => within(c).getByRole("link").getAttribute("aria-label"));
    expect(favs.sort()).toEqual(["Clannad", "Steins;Gate"]);

    await user.click(screen.getByRole("button", { name: "Clear filters" }));
    await user.selectOptions(screen.getByLabelText("Sort"), "title");
    await user.click(screen.getByRole("button", { name: /Descending/ }));
    const ordered = screen.getAllByTestId("game-card").map((c) => within(c).getByRole("link").getAttribute("aria-label"));
    expect(ordered[0]).toBe("Clannad");
    expect(ordered.at(-1)).toBe("Untitled doujin VN");
  });

  it("filters via the top-bar search and offers to clear when nothing matches", async () => {
    const user = userEvent.setup();
    renderApp("/");
    await screen.findAllByTestId("game-card");
    await user.type(screen.getByRole("searchbox", { name: "Search the library" }), "ひぐらし");
    await waitFor(() => expect(screen.getAllByTestId("game-card")).toHaveLength(1));
    await user.clear(screen.getByRole("searchbox", { name: "Search the library" }));
    await user.type(screen.getByRole("searchbox", { name: "Search the library" }), "no such game");
    expect(await screen.findByText("No games match")).toBeInTheDocument();
  });

  it("switches to the list view", async () => {
    const user = userEvent.setup();
    renderApp("/");
    await screen.findAllByTestId("game-card");
    await user.click(screen.getByRole("radio", { name: "List view" }));
    expect(screen.getAllByTestId("game-row")).toHaveLength(7);
  });

  it("shows an empty state that leads to the import wizard", async () => {
    mockState().games = [];
    const user = userEvent.setup();
    const { router } = renderApp("/");
    await user.click(await screen.findByRole("button", { name: "Add your first visual novel" }));
    expect(router.state.location.pathname).toBe("/import");
  });

  it("degrades gracefully when the backend command is not implemented", async () => {
    const { setMockOverride } = await import("../../lib/mock");
    setMockOverride("library_list_games", () => rejectWith({ code: "notImplemented", message: "library_list_games is not implemented", details: null }));
    renderApp("/");
    expect(await screen.findByText("Not available yet")).toBeInTheDocument();
  });
});
