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

## NVIDIA: black image, root cause and fix (verified)

Symptom: with the unmodified fork, `-F anime4k` was accepted but the output was entirely black on an RTX 5060 Ti
(driver 615.78). `-F linear` and `-F fsr` were fine in the same setups. NVIDIA is **not** incompatible: the fork had a bug
that only NVIDIA's limits expose.

| Experiment (vkcube, 640x360 -> 1280x720, mean brightness of gamescope's on-screen buffer) | linear | anime4k |
|---|---|---|
| headless backend | 0.132 | **0** |
| nested Wayland backend | 0.132 | **0** |
| `--backend sdl` (needs `-Dsdl2_backend=enabled`) | 0.133 | **0** |
| any backend + Khronos validation layer | 0.132 | **1.0 (white)** |
| headless, **with the alignment fix** | 0.132 | **0.132** |
| G-senjou through the Wine/Proton pipeline, with the fix | sharp edges vs. blurry linear edges | |

* The backend is irrelevant: SDL, Wayland and headless behave the same. (The white output reported in PR #2072 is what the
  validation layer produces; without it the same bug is black.) So the launcher does **not** switch backend for Anime4K.
* Not the shader maths and not the format declaration: a "copy the input" final pass was still black, and fixing the
  `rgba8` vs `rgba16f` declaration alone changed nothing.
* **Cause**, found with the Khronos validation layer (the Arch package `vulkan-validation-layers`, unpacked into a scratch
  folder, `VK_INSTANCE_LAYERS=VK_LAYER_KHRONOS_validation`): `VUID-VkWriteDescriptorSet-descriptorType-00327`,
  *"offset (16) must be a multiple of minUniformBufferOffsetAlignment (64)"*. Only Anime4K triggered it. gamescope's
  `uploadBufferData()` aligns constant-buffer offsets to a hard-coded 16 bytes; the Anime4K constants are 16 bytes,
  so consecutive dispatches got offsets 16, 32, ... which NVIDIA (alignment 64) cannot bind. The shader then read
  `u_inputSize = 0`, hit `if (coord.x >= u_inputSize.x) return;` and wrote nothing. The other filters use larger
  constant blocks that happen to fall on valid offsets. AMD/Intel limits are smaller, which hides the bug there.
* **Fix**: align to `max(16, minUniformBufferOffsetAlignment)` (`crates/vnl-graphics/patches/gamescope-anime4k-ubo-alignment.patch`,
  two lines in `rendervulkan.hpp/.cpp`). It does not touch any shader, so FSR, NIS and linear are unchanged: with the
  fix their output is the same as before (`linear` mean 0.132 in every run).
* The guided build applies the patch to its private clone (`git apply --check` first; skipped if the sources already contain
  it; if it no longer applies the build continues without it and says so in the log). The manifest records
  `patches: ["ubo-alignment"]`.

What the launcher does with this:

* The NVIDIA warning is shown only when the selected binary is not the managed build with the fix (custom and system
  binaries that list `anime4k` may not have it). The fallback to FSR/Linear stays available.
* Regression tests: `bundled_patch_targets_the_uniform_alignment`, `patch_that_does_not_apply_is_reported_but_does_not_break_the_build`,
  `nvidia_warning_is_not_shown_for_the_patched_managed_build`, and the real tests
  `real_build_of_the_anime4k_fork` + `real_anime4k_filter_renders_a_picture` (strict: the anime4k picture must not be black and
  must be close to the linear control). Run:
  `VNL_TEST_BUILD_HOME=<dir> cargo test -p vnl-graphics real_build -- --ignored --nocapture` then
  `... real_anime4k -- --ignored --nocapture` (needs `vkcube`, ImageMagick).

Remaining validation messages (also present with `-F linear` or harmless here), not changed: the shader declares `rgba8`
storage images for `rgba16f` layers (`Undefined-Value-StorageImage-FormatMismatch-ImageView`; the picture is correct
on this driver, but the spec calls the values undefined) and a uniform range larger than `maxUniformBufferRange`
(`...-00332`, also reported for the plain compositing path). They are worth reporting upstream.

## Distributions

* Arch: all build dependencies are in the official repos (`pacman -S --needed ...` is printed for what is missing).
* Debian/Ubuntu/Fedora: package names are provided on a best-effort basis (not run in CI here).
* Flatpak: building requires host toolchain access; use a custom binary path instead (see `docs/FLATPAK.md`).
