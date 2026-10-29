# Development guide

## 1. Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | stable, >= the workspace `rust-version` | Install with [rustup](https://rustup.rs). Needs `rustfmt` and `clippy` components. |
| Node.js + npm | Node >= 20.19 (Vite 8) | CI uses Node 24 LTS. |
| WebKitGTK 4.1, GTK 3, libsoup 3, librsvg, OpenSSL headers, pkg-config, C toolchain | distro packages | Per the [Tauri 2 Linux prerequisites](https://v2.tauri.app/start/prerequisites/). |

Run `scripts/dev-setup.sh` (or `make doctor`). It detects the distro, checks every build and
runtime dependency, and prints the install command for whatever is missing. It never runs
`sudo` itself and exits non-zero only when a required build dependency is missing.

### Arch Linux

```sh
sudo pacman -S --needed base-devel git curl wget file openssl pkgconf \
  webkit2gtk-4.1 gtk3 libsoup3 librsvg nodejs npm
# optional: xdotool (libxdo), libayatana-appindicator (only for a future tray icon)
```

### Debian 13 / Ubuntu 22.04+

```sh
sudo apt update
sudo apt install build-essential curl wget file pkg-config libssl-dev \
  libwebkit2gtk-4.1-dev libgtk-3-dev libsoup-3.0-dev librsvg2-dev libxdo-dev \
  libayatana-appindicator3-dev
# Node from the distro may be too old: use https://nodejs.org or nvm/fnm for Node >= 20.19.
```

### Fedora

```sh
sudo dnf install webkit2gtk4.1-devel openssl-devel curl wget file librsvg2-devel \
  libxdo-devel libappindicator-gtk3-devel nodejs npm
sudo dnf group install c-development
```

### openSUSE Tumbleweed

```sh
sudo zypper install webkit2gtk3-devel libopenssl-devel curl wget file librsvg-devel nodejs npm
sudo zypper install -t pattern devel_basis
```

(openSUSE's `webkit2gtk3-devel` provides the `webkit2gtk-4.1` pkg-config module.)

### rustup and PATH

rustup installs into `~/.cargo/bin`, which isn't on `PATH` for non-login shells or
for fish unless configured. Either run `source ~/.cargo/env` (bash/zsh),
`fish_add_path ~/.cargo/bin` (fish), or just use the `make` targets: the Makefile prepends
`$CARGO_HOME/bin` (default `~/.cargo/bin`) itself, so `make test` works without any shell setup.

## 2. Make targets

| Target | What it does |
|---|---|
| `make doctor` | `scripts/dev-setup.sh`: dependency check with install hints |
| `make setup` | `npm ci` and `cargo fetch --locked` |
| `make dev` | `npm run tauri dev` (Vite with HMR plus the Rust app, rebuilt on change) |
| `make dev-isolated` | Same, with `VNL_HOME=./.vnl-dev` (throwaway data, see below) |
| `make build` | Release binary without installers: `target/release/yomika` |
| `make test` | `cargo test --workspace` and `vitest run` |
| `make lint` | `cargo fmt --check`, `clippy -D warnings`, ESLint, `tsc --noEmit` |
| `make fmt` | `cargo fmt --all` |
| `make check` | Fast `cargo check --all-targets` + `tsc` |
| `make ci` | `scripts/check-all.sh`: everything CI runs, plus metadata validators |
| `make audit` | `npm audit` + `cargo deny check` (if installed) |
| `make bundle-appimage` / `bundle-deb` / `bundle-rpm` / `bundles` | Tauri installers in `target/release/bundle/` |
| `make arch-pkg` | Arch package from the committed `HEAD` (output in `target/makepkg/out`) |
| `make flatpak` | Flatpak build (needs flatpak-builder and generated sources; see `packaging/flatpak/README.md`) |
| `make clean-dev` | Delete `./.vnl-dev` |

`scripts/check-all.sh --fast` skips the slow release build; `--keep-going` runs every step and
lists all failures at the end.

Frontend only: `npm run dev` serves the UI at http://localhost:1420 in a normal browser. Outside
Tauri, `src/lib/api.ts` routes every command to the in-memory mock backend in `src/lib/mock/`
("demo mode"), which is handy for UI work without compiling Rust.

## 3. Isolated dev data (`VNL_HOME`)

By default the app uses the XDG directories (`AppPaths::from_env` in `crates/vnl-core/src/paths.rs`):

| Kind | Default location |
|---|---|
| Data (library DB, prefixes, runners, backups, logs) | `~/.local/share/vn-launcher/` |
| Config | `~/.config/vn-launcher/` |
| Cache (covers) | `~/.cache/vn-launcher/` |

If `VNL_HOME` is set and not empty, everything goes under `$VNL_HOME/{data,config,cache}`
instead. Use it for development so you never touch your real library or prefixes:

```sh
make dev-isolated                    # VNL_HOME=$PWD/.vnl-dev
make dev VNL_HOME=/tmp/vnl-scratch   # any directory
```

Tests must never use the real user directories: use `AppPaths::under(tempdir)` (or set
`VNL_HOME` to a temp dir), as required by `docs/CONTRACTS.md` rule 8.

## 4. Project layout

The authoritative layout, ownership and architecture rules are in
[`docs/CONTRACTS.md`](CONTRACTS.md). In short:

```
crates/vnl-core        shared types, AppPaths, AppError, events, CommandSpec
crates/vnl-runtime     runners, prefixes, command building, process sessions
crates/vnl-library     SQLite library, settings, playtime, VNDB, saves
crates/vnl-translate   text sources (hook/OCR), translation providers, pipeline
crates/vnl-graphics    gamescope, Wine FSR, upscaler, display + dependency checks
src-tauri/             Tauri shell: commands/<area>.rs, session.rs glue, state, event sink
src/contract/          TypeScript mirror of the Rust types + typed command map
src/                   React UI (pages, components, state, lib/api.ts, lib/mock)
packaging/ scripts/ docs/ .github/   tooling
```

Key rules: subsystem crates depend only on `vnl-core`; no Tauri types below `src-tauri`;
processes are built as `CommandSpec` (argv/env/cwd, never a shell); every settings struct is
`#[serde(default)]` and changes in Rust and TS together.

## 5. Adding a command end-to-end

Example: `library_toggle_favorite(game_id) -> Game`.

1. **Service method** in the owning crate (`crates/vnl-library/src/...`), returning
   `Result<T, vnl_core::AppError>`. Add unit tests with `AppPaths::under(tempdir)`.
2. **Shared types**: if the result or argument is a new type used by several areas, put it in
   `vnl-core`; otherwise keep it in the subsystem crate. Derive `Serialize`/`Deserialize` with
   `#[serde(rename_all = "camelCase")]` to match the TS side.
3. **Tauri command** in `src-tauri/src/commands/library.rs`:

   ```rust
   #[tauri::command]
   pub async fn library_toggle_favorite(state: State<'_, AppState>, game_id: i64) -> Result<Game, AppError> {
       state.library.toggle_favorite(GameId(game_id))
   }
   ```

   Rust parameters are `snake_case`; Tauri maps the JS `camelCase` keys (`gameId`).
   Blocking work (filesystem scans, archives) goes through `spawn_blocking`.
4. **Register it** inside your area's block of `tauri::generate_handler![...]` in
   `src-tauri/src/lib.rs`.
5. **TypeScript contract**: add the entry to the area's command map in
   `src/contract/library.ts`:

   ```ts
   library_toggle_favorite: { args: { gameId: GameId }; result: Game };
   ```

6. **Mock backend**: implement it in `src/lib/mock/backend.ts` so demo mode and UI tests keep
   working.
7. **Call it** from the UI with the typed wrapper: `await call("library_toggle_favorite", { gameId })`.
   Errors reject with `AppError { code, message, details }`.
8. **Plugin permissions**: only if the command needs a Tauri plugin capability, add it to
   `src-tauri/capabilities/default.json`. Custom commands registered via `generate_handler!`
   need no capability entry.
9. Run `make lint test` before pushing.

## 6. Formatting and lint configuration

- `rustfmt.toml`: `max_width = 120`, `use_small_heuristics = "Max"`. Run `make fmt`.
- `clippy.toml`: allows unwrap/expect/panic in tests, raises the argument-count threshold.
  CI runs clippy with `-D warnings`.
- `eslint.config.js`: flat config, `@eslint/js` recommended, type-aware `typescript-eslint`
  recommended rules (project service), `eslint-plugin-react-hooks` v7 recommended. Unused
  variables prefixed with `_` are allowed.
- `.editorconfig`: 2 spaces by default, 4 for Rust/TOML, tabs in the Makefile.
- `deny.toml`: cargo-deny license allow-list, RustSec advisories, bans and sources.
  Install with `cargo install --locked cargo-deny`.

## 7. Logs and debugging

- App logs: `~/.local/share/vn-launcher/logs/` (or `$VNL_HOME/data/logs/`). They're also
  streamed to the UI's log viewer via the `vnl://log` event.
- `VNL_LOG=debug make dev` raises the tracing level (`EnvFilter` syntax, e.g. `VNL_LOG=info,vnl_runtime=trace`).
- Right-click, then "Inspect Element", opens the WebKit inspector in debug builds.
- WebKitGTK rendering problems (blank window, flicker): see
  [TROUBLESHOOTING.md](TROUBLESHOOTING.md#blank-or-white-window-webkitgtk).

## Debugging helpers

| Variable | Effect |
|---|---|
| `VNL_HOME=/some/dir` | Keep all data/config/cache under that directory (isolated dev/test data). |
| `VNL_LOG=debug` | Tracing filter for the backend (`info` by default). |
| `VNL_INITIAL_ROUTE=/diagnostics` | Open that page at startup (smoke tests, screenshots). |
| `VNL_VNDB_URL=http://127.0.0.1:8080` | Point the VNDB client at a mock server. |
| `VNL_SECRET_BACKEND=file` | Store provider API keys in the 0600 file fallback instead of the Secret Service. |

Note: a plain `cargo build --release` produces a binary that loads the Vite
dev server (`devUrl`). Release binaries must be built through the Tauri CLI
(`npm run tauri build`, `make build`), which enables `tauri/custom-protocol`
and embeds `dist/`.

### Naming and existing installations

The application and executable are named Yomika (`yomika`). The internal `vnl-*`
crate names, `VNL_*` environment variables, XDG `vn-launcher` storage directories
and `vn-launcher` keyring service are retained for compatibility with existing data.
The application ID is now `io.github.yomika.Yomika`; existing Flatpak installations
use a separate sandbox under the former `io.github.vnlauncher.VNLauncher` ID.
