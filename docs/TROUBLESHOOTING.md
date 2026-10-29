# Troubleshooting

Start with `scripts/dev-setup.sh --runtime-only`. It reports missing runners, locale, fonts,
OCR data and so on, and prints the install commands.

## Where are the logs?

| Install type | Log directory |
|---|---|
| Native (AppImage, deb, rpm, Arch package, source) | `~/.local/share/vn-launcher/logs/` |
| Dev with `VNL_HOME` | `$VNL_HOME/data/logs/` |
| Flatpak | `~/.var/app/io.github.vnlauncher.VNLauncher/data/vn-launcher/logs/` |

Application tracing goes to stderr. Run from a terminal with `VNL_LOG=debug vn-launcher`
(filter syntax like `VNL_LOG=info,vnl_runtime=trace`). Wine's own debug output is controlled
per game with `WINEDEBUG` in the game's environment settings (e.g. `WINEDEBUG=+loaddll`, or
`-all` to silence it). For Proton add `PROTON_LOG=1`; the log is written to
`~/steam-<appid>.log` (umu uses `umu-<id>` or `0`).

## Wine / Proton prefixes

**The game doesn't start at all / "no runner".** Check that a runner was detected
(Settings → Runners). System Wine needs `wine` on `PATH`. Proton needs either
[umu-launcher](#installing-umu-launcher) or a Proton build in
`~/.steam/steam/compatibilitytools.d/` / `steamapps/common/Proton*`.

**First launch takes minutes.** Creating a prefix (`wineboot`) and, for Proton, downloading
the Steam Runtime via umu happens once per prefix/runtime. Watch the task progress in the UI.

**"Bad EXE format" / 32-bit games fail.** 32-bit games need a WoW64-capable Wine (current
Wine builds handle this) and 32-bit graphics drivers. On Arch enable `[multilib]` and install
`lib32-vulkan-icd-loader` plus your driver (`lib32-mesa`/`lib32-vulkan-radeon`/`lib32-nvidia-utils`).
On Debian/Ubuntu: `sudo dpkg --add-architecture i386` first.

**Mixing runners in one prefix.** A prefix created by Proton 9 and then opened with system
Wine 11 (or the other way round) gets "updated" and may break. Keep one runner per prefix, or
let the launcher create a separate prefix per runner. VN Launcher never deletes or resets a
prefix automatically; to start over, rename the prefix directory yourself (under
`~/.local/share/vn-launcher/prefixes/`) and the next launch creates a fresh one.

**Missing runtimes (d3dx9, vcrun, quartz, wmp, xact).** Many older VN engines need them. Use
the winetricks action on the game page or run inside the prefix yourself:
`WINEPREFIX=<prefix> winetricks d3dx9 vcrun2010`. For Proton prefixes, the Wine prefix is
`<prefix>/pfx` and you should use `umu-run winetricks ...` with the same `WINEPREFIX`/`PROTONPATH`.

**Videos (opening movies) are black or the game hangs at the intro.** Media Foundation /
WMV playback is a common VN problem. Proton (GE-Proton in particular) handles more formats
than plain Wine. Try GE-Proton, or install `wmp9`/`quartz`/`lavfilters` via winetricks for
system Wine.

## Japanese locale (`ja_JP.UTF-8`)

Many Japanese games read the ANSI code page from the locale and show mojibake or crash
without it. Set the game's locale to Japanese (the launcher sets `LANG`/`LC_ALL=ja_JP.UTF-8`),
which only works if the locale is generated on the host:

```sh
locale -a | grep -i ja_JP     # should print ja_JP.utf8
```

- **Arch / Manjaro**: `sudo sed -i 's/^#ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen`
- **Debian**: `sudo sed -i 's/^# *ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen`
  (or `sudo dpkg-reconfigure locales`)
- **Ubuntu**: `sudo locale-gen ja_JP.UTF-8` (install `language-pack-ja` if it isn't available)
- **Fedora**: `sudo dnf install glibc-langpack-ja`
- **openSUSE**: `sudo zypper install glibc-locale`

You don't need to change your desktop language; the variable is set only for the game process.

## CJK fonts and "fakejapanese"

Symptoms: squares (tofu) or wrong-looking glyphs in game menus, or the game picks a
proportional font and the text overflows.

1. Install CJK fonts on the host: `noto-fonts-cjk` (Arch), `fonts-noto-cjk` (Debian/Ubuntu),
   `google-noto-sans-cjk-fonts` (Fedora).
2. Inside the prefix, Japanese games often ask for MS Gothic / MS Mincho / MS UI Gothic, which
   Wine doesn't ship. Options:
   - `winetricks fakejapanese` maps those names to installed free fonts (Takao/IPA/Noto) via
     registry font replacements. `fakejapanese_ipamona` and `fakejapanese_vlgothic` are variants.
   - `winetricks cjkfonts` installs a broader set.
   - If you own a Windows install, copying `msgothic.ttc` into `<prefix>/drive_c/windows/Fonts`
     gives the most faithful rendering (not redistributable, so we don't automate it).
3. If text renders but is garbled (mojibake), it's the locale, not fonts; see above.

## gamescope

**Nested gamescope on GNOME / KDE.** gamescope runs as a nested Wayland compositor inside your
session. Requirements and common fixes:

- Use a current gamescope (3.14+). On NVIDIA you need the proprietary driver 555+ with
  `nvidia-drm.modeset=1`; older drivers fail with Vulkan/DRM format errors.
- **GNOME (Mutter)** works nested; if the window is black, try `--backend sdl` (gamescope
  3.15+) or disable the gamescope WSI layer with `ENABLE_GAMESCOPE_WSI=0`.
- **KDE Plasma 6**: nested mode works. If you see stutter, disable "Allow tearing" and
  adaptive sync for the nested window, or try `--backend wayland`.
- **Mouse is stuck / pointer offset**: add `--force-grab-cursor`.
- **Game window is tiny / not scaled**: set both the game resolution (`-w/-h`) and the output
  resolution (`-W/-H`), and choose an upscaler filter (`-F fsr` or `-F nis`).
- **No sound difference / audio issues** aren't caused by gamescope; check PipeWire.
- gamescope's `--hdr-enabled` and `--adaptive-sync` need a session/driver that supports them.
  Leave them off unless you know they work.

Disable gamescope for the game to rule it out; most VNs don't need it, and Wine FSR
(`WINE_FULLSCREEN_FSR=1`, Proton/GE only) is a lighter alternative for fullscreen upscaling.

## Blank or white window (WebKitGTK)

The UI is a WebKitGTK webview. If the window stays white/blank, flickers, or crashes on start:

| Symptom | Try |
|---|---|
| Blank window on NVIDIA (proprietary driver), Wayland | `WEBKIT_DISABLE_DMABUF_RENDERER=1 vn-launcher` |
| Blank/garbled window in VMs or with broken GL | `WEBKIT_DISABLE_COMPOSITING_MODE=1 vn-launcher` |
| `Failed to create GBM buffer` / EGL errors | `WEBKIT_DISABLE_DMABUF_RENDERER=1`, or force X11: `GDK_BACKEND=x11 vn-launcher` |
| AppImage only: EGL/Mesa errors, crash at start | The AppImage bundles WebKitGTK from the build host; prefer the distro package / PKGBUILD, or try `WEBKIT_DISABLE_DMABUF_RENDERER=1` (see [PACKAGING.md](PACKAGING.md#appimage)) |
| Text is blurry with fractional scaling | `GDK_SCALE`/`GDK_DPI_SCALE`, or run under XWayland with `GDK_BACKEND=x11` |

To make an override permanent, copy the `.desktop` file to `~/.local/share/applications/` and
prefix `Exec=` with `env WEBKIT_DISABLE_DMABUF_RENDERER=1`.

## Installing umu-launcher

umu-launcher runs Proton outside Steam with the Steam Runtime container, the same way Steam
does. That's the recommended runner for Proton builds.

- **Arch**: `sudo pacman -S umu-launcher` (in `[multilib]`)
- **Fedora**: `sudo dnf install umu-launcher`
- **Debian/Ubuntu, others**: download the `.deb` or the zipapp from the
  [releases page](https://github.com/Open-Wine-Components/umu-launcher/releases). The zipapp
  can be extracted anywhere on `PATH` (e.g. `~/.local/bin/umu-run`).
- **NixOS**: `umu-launcher` in nixpkgs.

Verify with `umu-run --help`. The first run downloads the Steam Runtime (~200 MB) into
`~/.local/share/umu`. If you want to use a specific Proton, set it in the runner settings
(it becomes `PROTONPATH`). Otherwise umu downloads the latest UMU-Proton.

## OCR and translation

- `tesseract --list-langs` must list `jpn` (and `jpn_vert` for vertical text). Install
  `tesseract-data-jpn` (Arch), `tesseract-ocr-jpn` (Debian/Ubuntu), `tesseract-langpack-jpn` (Fedora).
- Ollama must be running (`systemctl --user start ollama` or `ollama serve`) and the model
  pulled (`ollama pull <model>`). The default endpoint is `http://127.0.0.1:11434`.
- Online providers (DeepL, OpenAI-compatible) only receive text after you configure them.

## Flatpak-specific issues

See [FLATPAK.md](FLATPAK.md): games run on the host through `flatpak-spawn --host`, so Wine,
umu and gamescope must be installed on the **host**, not in the sandbox.
