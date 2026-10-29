# Packaging

| Format | How | Status |
|---|---|---|
| Release binary (no installer) | `make build` → `target/release/vn-launcher` | Used by the Arch package |
| AppImage | `make bundle-appimage` (Tauri bundler) | Configured. Release workflow builds it on Ubuntu 22.04. Not yet tested on other distros |
| deb | `make bundle-deb` | Configured, built in CI (`tauri-build` job) |
| rpm | `make bundle-rpm` | Configured |
| Arch Linux | `packaging/arch/PKGBUILD` (`vn-launcher-git`), `make arch-pkg` | `makepkg -o` (fetch + prepare + pkgver) verified. Full package build not run yet |
| Flatpak | `packaging/flatpak/`, `make flatpak` | Manifest only; see [FLATPAK.md](FLATPAK.md) |

Tauri bundles are configured in `src-tauri/tauri.conf.json` (`bundle.targets = appimage, deb,
rpm`, identifier `io.github.vnlauncher.VNLauncher`, binary `vn-launcher`). Output lands in
`target/release/bundle/<format>/`.

## Common metadata

`packaging/linux/` holds the files the PKGBUILD and Flatpak install:

- `io.github.vnlauncher.VNLauncher.desktop`: validated with `desktop-file-validate`.
- `io.github.vnlauncher.VNLauncher.metainfo.xml`: AppStream, validated with
  `appstreamcli validate --no-net --pedantic`. The only pedantic hint is the uppercase letters
  in the (fixed) app ID. Replace the `example.invalid` URLs and the placeholder screenshot,
  and set `<project_license>` once the license is chosen.
- Icons come from `src-tauri/icons` (32, 64, 128, 256 (`128x128@2x`), 512 (`icon.png`)) and are
  installed as `hicolor/<size>/apps/io.github.vnlauncher.VNLauncher.png`.

The Tauri deb/rpm/AppImage bundles generate their **own** `.desktop` file named after
`productName` (`VN Launcher.desktop`, `Icon=vn-launcher`) and don't install the metainfo file.
To make the bundles use the same reverse-DNS IDs as the distro packages, add to
`tauri.conf.json` → `bundle.linux` (orchestrator-owned file):

```json
"linux": {
  "deb":  { "desktopTemplate": "../packaging/linux/io.github.vnlauncher.VNLauncher.desktop",
            "files": { "/usr/share/metainfo/io.github.vnlauncher.VNLauncher.metainfo.xml": "../packaging/linux/io.github.vnlauncher.VNLauncher.metainfo.xml" } },
  "rpm":  { "desktopTemplate": "../packaging/linux/io.github.vnlauncher.VNLauncher.desktop",
            "files": { "/usr/share/metainfo/io.github.vnlauncher.VNLauncher.metainfo.xml": "../packaging/linux/io.github.vnlauncher.VNLauncher.metainfo.xml" } }
}
```

(Not applied yet. The keys exist in the Tauri 2 config schema shipped with `@tauri-apps/cli`.
Check the generated package with `dpkg-deb --contents` afterwards, since Tauri may still name
the `.desktop` file and icon after the product/binary.)

## AppImage

### How Tauri builds it

`tauri build --bundles appimage`:

1. Builds the release binary (frontend embedded via the `custom-protocol` feature).
2. Downloads its own tooling into `~/.cache/tauri/`: `linuxdeploy-x86_64.AppImage`, the
   `linuxdeploy-plugin-gtk` script, `linuxdeploy-plugin-appimage` and the AppImage runtime.
   No system `appimagetool` is needed. Network access is needed on the first build.
3. linuxdeploy copies the binary and **all shared-library dependencies from the build host**
   (WebKitGTK, GTK 3, GLib, libsoup, ...) into `AppDir/usr/lib`, plus the GTK plugin's
   loaders/schemas, and patches RPATHs. Optionally (`bundle.linux.appimage.bundleMediaFramework`)
   GStreamer for audio/video in the webview.
4. Packs it into a squashfs with the AppImage runtime.

The linuxdeploy tools are AppImages themselves, so the **build** needs FUSE 2 (`libfuse.so.2`)
or `APPIMAGE_EXTRACT_AND_RUN=1`, which makes them extract to a temp dir instead of mounting.
The release workflow sets it, since GitHub runners have no FUSE. Users who can't mount
AppImages (no `fuse2`, containers) can run ours with `./VN_Launcher.AppImage --appimage-extract-and-run`
or by setting `APPIMAGE_EXTRACT_AND_RUN=1`.

### Build host matters

The AppImage runs only on systems whose glibc is **at least as new** as the build host's, and
it carries that host's WebKitGTK. So the release workflow builds on **Ubuntu 22.04** (glibc 2.35)
rather than 24.04. Building it on Arch produces an AppImage that only runs on very new
distros. Use the PKGBUILD there instead.

Known Arch issue: linuxdeploy's bundled `strip` is too old for recent toolchains' ELF sections
(`.relr.dyn`) and fails with "unknown type [0x13] section `.relr.dyn'". Workaround:
`NO_STRIP=true make bundle-appimage`.

### Known WebKitGTK problems in AppImages

| Problem | Cause | Workaround |
|---|---|---|
| Blank/white window on NVIDIA proprietary drivers (esp. Wayland) | WebKitGTK's DMA-BUF renderer vs. the driver's GBM/EGL | `WEBKIT_DISABLE_DMABUF_RENDERER=1` |
| `EGL_BAD_ALLOC`, `Could not create default EGL display`, crash at start | Bundled libs (libEGL/libgbm/Mesa pieces pulled in by linuxdeploy) mismatch the host GPU driver | Make sure no Mesa/GL libs are bundled (linuxdeploy excludes most via its excludelist). Otherwise run with `WEBKIT_DISABLE_COMPOSITING_MODE=1` |
| WebKit child processes fail (`WebKitWebProcess` not found) | WebKitGTK spawns helper executables via absolute paths compiled into libwebkit | Tauri's AppImage handling patches this. If it breaks with a new WebKit, prefer distro packages |
| No video/audio in the webview | GStreamer not bundled | `bundleMediaFramework: true` (bigger AppImage). Not needed for our UI |
| Fonts/theme look wrong | GTK theme and fontconfig come from the host, but bundled GTK may not match the host theme engine | Cosmetic; `GTK_THEME=Adwaita` |
| File dialogs ignore portals / look old | Bundled GTK 3 + no `xdg-desktop-portal-gtk` integration | Install `xdg-desktop-portal-gtk` on the host |

General advice: AppImage is the "works on many distros" fallback. For daily use, distro
packages (PKGBUILD / deb / rpm) link the system WebKitGTK and avoid all of the above.

## deb / rpm

- deb depends on `libwebkit2gtk-4.1-0`, `libgtk-3-0` (Tauri fills these in). Runtime tools
  (Wine, umu, gamescope, ...) aren't declared as dependencies. They could be added as weak
  dependencies via `bundle.linux.deb.recommends` / `bundle.linux.rpm.recommends` (both exist in
  the Tauri 2 config schema), e.g. `["wine", "winetricks", "gamescope", "tesseract-ocr-jpn"]`
  for deb. Package names differ per distro, so keep the list short.
- rpm is built by Tauri's pure-Rust rpm writer. `rpmbuild` isn't needed. Test on Fedora with
  `rpm -qpR target/release/bundle/rpm/*.rpm`.

## Arch Linux (PKGBUILD)

`packaging/arch/PKGBUILD` builds `vn-launcher-git`:

- `prepare()`: `npm ci` (cache in `$srcdir/npm-cache`) and `cargo fetch --locked`.
- `build()`: `npm run tauri -- build --no-bundle --ci -- --frozen` (frontend build + release
  binary with embedded assets, no network).
- `check()`: `cargo test --frozen --workspace --release`.
- `package()`: binary, desktop file, metainfo, hicolor icons, README, and LICENSE when it exists.
- `depends`: exactly the binary's `DT_NEEDED` libraries (webkit2gtk-4.1, gtk3, libsoup3, glib2,
  cairo, gdk-pixbuf2, dbus, libgcc, glibc) plus hicolor-icon-theme. `optdepends` lists wine, umu-launcher, winetricks,
  gamescope, tesseract(+jpn), ollama, imagemagick, wl-clipboard/xclip, noto-fonts-cjk and the
  lib32 Vulkan pieces.
- `options=('!lto')`: Rust does its own (thin) LTO. makepkg's `-flto=auto` CFLAGS only slow
  down C deps built by cc-rs (e.g. bundled SQLite).

Local build: `make arch-pkg`. It sets `VNL_REPO=file://$PWD`, so makepkg clones **the committed
HEAD** of your checkout (uncommitted changes aren't included). Output goes to
`target/makepkg/out/`. Missing dependencies are reported. `make arch-pkg MAKEPKG_FLAGS=-s`
lets makepkg install them via sudo pacman. If Rust comes from rustup rather than the `rust`
package, pacman doesn't know about `cargo`. Use `MAKEPKG_FLAGS=-d` (skip dependency checks).

After editing the PKGBUILD: `make arch-srcinfo` (CI's Arch job fails if `.SRCINFO` is stale).
For the AUR, replace `url`/`source` with the real repository and push `PKGBUILD` + `.SRCINFO`
to `ssh://aur@aur.archlinux.org/vn-launcher-git.git`. Run `namcap` on both the PKGBUILD and the
built package first (`namcap` wasn't available where this was written).

## Release workflow

`.github/workflows/release.yml` runs on `v*` tags (and manually):

1. Checks that the tag matches `tauri.conf.json` `version`.
2. Installs the Tauri deps on **ubuntu-22.04**, builds `appimage,deb,rpm` with
   `APPIMAGE_EXTRACT_AND_RUN=1`.
3. Writes `SHA256SUMS`, smoke-tests the AppImage (`--appimage-extract`, `ldd` has no
   missing libraries) and uploads everything as a workflow artifact.
4. For tags: creates a **draft** GitHub release with generated notes and the files attached.
   Review and publish it by hand. Tags containing `-` (e.g. `v0.2.0-rc.1`) are marked as
   prereleases.

Not covered yet: code signing/GPG signatures for artifacts, Tauri updater artifacts
(`createUpdaterArtifacts`), aarch64 builds (add an `ubuntu-22.04-arm` matrix entry).

## CI

`.github/workflows/ci.yml`:

| Job | Runs |
|---|---|
| `rust` (ubuntu-24.04) | `cargo fmt --check`, `clippy --all-targets -D warnings`, `cargo test --workspace` |
| `frontend` | `npm ci`, `tsc`, ESLint, vitest, `vite build` |
| `tauri-build` | `tauri build --bundles deb` smoke build + `dpkg-deb` inspection, uploads the deb |
| `arch` (`archlinux:latest` container) | `cargo test --workspace`, PKGBUILD/.SRCINFO consistency |
| `hygiene` | shellcheck, actionlint, desktop-file-validate, appstreamcli |
| `cargo-deny` | licenses/bans/sources (blocking), advisories (non-blocking) |

Local equivalent: `make ci` (`scripts/check-all.sh`).
