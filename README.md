# VN Launcher

A Linux-first launcher for visual novels. It keeps a library of your VNs, runs Windows titles
through Wine, Proton or umu-launcher with per-game prefixes and Japanese locale settings, and
can extract and translate in-game text (hooking, clipboard or OCR) with a local or online
translation backend.

Built with [Tauri 2](https://v2.tauri.app/) (Rust backend, WebKitGTK webview) and React 19 +
TypeScript.

> **Status: pre-alpha (0.1.0, under active development).** Interfaces, data formats and
> settings may change without migration. Don't point it at a Wine prefix you can't afford to
> lose; the launcher never deletes or resets prefixes on purpose, but it hasn't been tested
> in real use yet.

<!-- Screenshots: add docs/screenshots/library.png etc. once the UI settles. -->
_Screenshots will be added once the UI is stable._

## Features

The status column describes the code in this repository today, not plans. "Backend" means
the Rust crate implements and unit-tests the feature but the UI wiring or end-to-end testing
against real games isn't finished.

| Area | Feature | Status |
|---|---|---|
| Library | Add games, folder scan, search/sort, play status | Backend + UI in progress |
| Library | VNDB metadata and covers | Backend |
| Library | Playtime tracking per session | Backend |
| Library | Save-game location tracking, backup and restore (with confirmation) | Backend |
| Runtime | Wine runner detection (system, custom builds) | Backend |
| Runtime | Proton via umu-launcher, Steam `compatibilitytools.d` discovery | Backend |
| Runtime | Per-game prefixes, `LANG=ja_JP.UTF-8` / locale, env vars, DLL overrides | Backend |
| Runtime | winetricks verbs, auxiliary programs inside the game prefix | Backend |
| Graphics | gamescope wrapping (resolution, FSR/NIS, fullscreen) | Backend |
| Graphics | Wine fullscreen FSR (`WINE_FULLSCREEN_FSR`) | Backend |
| Graphics | External real-time upscaler companion process | Backend, experimental |
| Graphics | Display and dependency checks (gamescope, Vulkan, umu, fonts, locale) | Backend |
| Translation | OCR text source (Tesseract, Ollama vision) with capture region | Backend |
| Translation | Text hooking via an external hook host (LunaHook-style, run inside the prefix) | In progress |
| Translation | Clipboard text source | Planned |
| Translation | Providers: Ollama (local), OpenAI-compatible, DeepL | Backend |
| Translation | Translation overlay/log window | UI in progress |
| Packaging | AppImage, deb, rpm (Tauri bundler), Arch PKGBUILD | Configured, not release-tested |
| Packaging | Flatpak | Manifest only, limited by the sandbox (see [docs/FLATPAK.md](docs/FLATPAK.md)) |

Privacy: no dialogue text leaves your machine unless you configure an online translation
provider yourself.

## Quick start (from source)

You need Rust (via [rustup](https://rustup.rs)), Node.js >= 20.19 with npm, and the
Tauri/WebKitGTK development packages. `scripts/dev-setup.sh` checks all of this and prints
the exact install commands for Arch, Debian/Ubuntu, Fedora and openSUSE. It never runs sudo.

```sh
scripts/dev-setup.sh      # or: make doctor
make setup                # npm ci + cargo fetch
make dev-isolated         # run with throwaway data in ./.vnl-dev
make dev                  # run with your real data dir (~/.local/share/vn-launcher)
```

Other targets: `make test`, `make lint`, `make build`, `make bundle-appimage`, `make arch-pkg`.
Run `make help` for the full list. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md).

### Runtime dependencies

None of these are bundled. Install the ones you need:

| Program | Used for | Required? |
|---|---|---|
| Wine, or Proton + [umu-launcher](https://github.com/Open-Wine-Components/umu-launcher) | Running Windows games | Required for Windows games |
| winetricks | Installing runtimes/fonts into prefixes | Optional |
| gamescope | Nested compositor, fixed resolution, FSR | Optional |
| Tesseract + Japanese data | OCR text source | Optional |
| Ollama | Local LLM translation / vision OCR | Optional |
| ImageMagick | OCR preprocessing | Optional |
| wl-clipboard / xclip | Clipboard text source | Optional |
| `ja_JP.UTF-8` locale, CJK fonts | Many Japanese games | Strongly recommended |

## Documentation

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): setup, make targets, project layout, adding a command
- [docs/CONTRACTS.md](docs/CONTRACTS.md): architecture and module ownership rules
- [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md): Wine/Proton, locale, fonts, gamescope, blank window
- [docs/PACKAGING.md](docs/PACKAGING.md): AppImage, deb/rpm, Arch, release workflow
- [docs/FLATPAK.md](docs/FLATPAK.md): Flatpak sandbox limitations and security trade-offs
- [docs/LICENSES.md](docs/LICENSES.md): dependency licenses and external tools

## License

**Not decided yet.** The workspace currently uses the placeholder `LicenseRef-TBD`, so no
license is granted at this point. [docs/LICENSES.md](docs/LICENSES.md) lists the options and
their trade-offs. Third-party dependencies keep their own licenses.

## Anime4K upscaling

Optional `-F anime4k` filter through the gamescope-anime4k fork, with a guided private build. See [docs/GAMESCOPE_ANIME4K.md](docs/GAMESCOPE_ANIME4K.md) (includes a known black-image issue on NVIDIA).
