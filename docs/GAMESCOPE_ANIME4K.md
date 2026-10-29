# Anime4K upscaling (gamescope-anime4k fork)

Anime4K is an optional gamescope upscaling filter (`-F anime4k`, Anime4K 2x CNN UL, implemented as
Vulkan compute shaders). It exists only in the fork
[Et3rnos/gamescope-anime4k](https://github.com/Et3rnos/gamescope-anime4k)
(upstream PR [ValveSoftware/gamescope#2072](https://github.com/ValveSoftware/gamescope/pull/2072)).
Standard gamescope, FSR, NIS, SGSR and the other filters keep working without it.

## Which gamescope is used

Binaries are discovered and inspected (`--help` lists the real `-F` values), then chosen per game:

| Order | Source | Where it comes from |
|---|---|---|
| 1 | This game | Graphics tab > "Gamescope binary for this game" (`GamescopeConfig.binaryPath`) |
| 2 | Custom | Settings > Gamescope > custom path (`AppSettings.gamescopePath`) |
| 3 | System | `PATH` (or the Flatpak gamescope extension) |
| 4 | Anime4K build | `tools/gamescope-anime4k/bin/gamescope`, built by the launcher |

* For `-F anime4k` the first binary that lists `anime4k` wins (game, custom, Anime4K build, system).
  If there is none, launching fails with a message pointing to Settings > Gamescope. It never silently
  falls back to a gamescope that would exit with "unknown filter".
* Every other filter uses the order above and skips binaries that do not list it (e.g. the fork has no `sgsr`).
* Monitor, resolution, fullscreen, scaler, FPS and the fullscreen assist are independent of the binary.

## Guided build (opt-in, no root)

Settings > Gamescope > "Download and build". Nothing is built unless you press it.

1. **Dependencies are checked first** (tools: git, meson, ninja, g++, pkg-config, glslang, wayland-scanner;
   libraries via `pkg-config`: Vulkan, Wayland, libdrm >= 2.4.113, libinput, libseat, libdecor, luajit, X11/XCB
   modules, ...). Missing items are listed with the install command for Arch, Debian/Ubuntu and Fedora.
   The app shows these commands but never runs them (no sudo).
2. Sources go to `~/.cache/vn-launcher/gamescope-anime4k/src` (shallow clone, six needed submodules,
   OpenVR skipped; submodule fetches are retried).
3. `meson setup` with `--buildtype=release`, OpenVR/PipeWire/AVIF/SDL2/benchmark disabled and
   `-Dwerror=false` (wlroots builds with `-Werror`, which breaks as soon as a system library such as libinput
   adds an enum value; this happened on Arch), then `ninja`.
4. `meson install --destdir <staging>`, the result is verified (`--help` must list `anime4k`) and only then
   swapped into `~/.local/share/vn-launcher/tools/gamescope-anime4k`. A failed rebuild keeps the previous build.
   `/usr/bin/gamescope` is never touched.
5. Progress (`vnl://task`, id `gamescope-build`), output (log source `gamescope-build`, also saved to
   `logs/gamescope-build.log`) and **Cancel** (terminates the whole process group) are available in the UI.

Tested: full build on Arch Linux (gcc 16, libinput with `LIBINPUT_SWITCH_KEYPAD_SLIDE`) in about 4 minutes cold and 30 s
incremental; fake-toolchain tests cover progress, retry, cancellation, failure and "previous build kept".

## Known issue: black image on NVIDIA (verified)

With the NVIDIA proprietary driver (RTX 5060 Ti, 615.78) the fork accepts `-F anime4k` and keeps running, but the
output is **entirely black**, in headless and nested mode, with a Wine game (G-senjou) and with `vkcube`; `-F linear`
and `-F fsr` render normally in the same setup. Findings so far:

* not caused by the shader maths: replacing the last pass by "copy the input" is still black;
* not caused by the storage image format declaration (`rgba8` in `descriptor_set.h` vs. float16 layers): fixing it
  in a local patch did not change the result.

So the problem is in the fork's Anime4K output path on this driver. The launcher shows a warning when the NVIDIA
driver is detected and Anime4K is selected. Reproduce with
`VNL_TEST_BUILD_HOME=<dir> cargo test -p vnl-graphics real_build -- --ignored --nocapture` followed by
`cargo test -p vnl-graphics real_anime4k -- --ignored --nocapture` (set `VNL_STRICT_ANIME4K=1` to fail on black output).
It is untested on AMD/Intel Mesa drivers.

## Distributions

* Arch: all build dependencies are in the official repos (`pacman -S --needed ...` is printed for what is missing).
* Debian/Ubuntu/Fedora: package names are provided on a best-effort basis (not run in CI here).
* Flatpak: building requires host toolchain access; use a custom binary path instead (see `docs/FLATPAK.md`).
