# License compliance review

Scope: the licenses of the code we **link or bundle** (Rust crates, npm packages that end up in
the frontend bundle), the external programs we **invoke**, and the options for the project's own
license. This is an engineering review, not legal advice.

How to regenerate the dependency section:

```sh
scripts/license-report.py > /tmp/report.md   # Markdown tables (paste below the marker)
scripts/license-report.py --check             # CI-style: fails on shipped strong-copyleft/unknown
cargo deny check licenses bans sources        # policy in deny.toml (allow-list)
npm audit                                     # npm advisories
```

`license-report.py` reads `cargo metadata --filter-platform x86_64-unknown-linux-gnu` and
counts crates reachable from the workspace through normal **and build** edges as "shipped".
That's conservative, since proc-macros and build scripts don't end up in the binary. npm data comes
from `package-lock.json`. Only non-dev (`dependencies`) packages count as shipped, because Vite
bundles them into `dist/`.

## Summary

| Area | Result |
|---|---|
| Rust, shipped (≈420 crates) | All permissive (MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode-3.0 / CDLA-Permissive-2.0) **except 5 MPL-2.0 crates** |
| MPL-2.0 crates | `cssparser`, `cssparser-macros`, `dtoa-short`, `selectors` (Servo CSS stack via `tauri-utils` → `dom_query`, used for HTML/CSP processing, mostly at build time in `tauri-codegen`) and `option-ext` (via `directories` → `dirs-sys`, in `vnl-core`) |
| npm, shipped (13 packages: React, React Router, Tauri JS API/plugins) | All MIT / ISC / Apache-2.0 / MIT OR Apache-2.0 |
| npm, dev-only | Permissive plus `lightningcss` (MPL-2.0, Vite's CSS tooling). Build tool only, not shipped |
| GPL / AGPL / LGPL in linked code | **None** |
| Unknown / missing license | **None** |
| System libraries (dynamically linked, not bundled except in the AppImage) | WebKitGTK (LGPL-2.1 / BSD), GTK 3, GLib, libsoup 3, cairo, pango (LGPL-2.1+ / MPL-1.1), glibc (LGPL-2.1+) |

### What MPL-2.0 means for us

MPL-2.0 is a **file-level** copyleft. We can combine these crates with code under any license,
including proprietary. Obligations when distributing binaries: tell recipients where the
source of the MPL-covered files is (crates.io is fine, as we use them unmodified) and keep
their license notices. If we ever **modify** those files, the modified files must stay MPL-2.0
and their source must be available. MPL-2.0 is GPL-compatible (no "Incompatible With Secondary
Licenses" notice on these crates), so it doesn't block any of the project license options below.

### LGPL system libraries and the AppImage

Native packages link WebKitGTK/GTK/GLib dynamically from the system, so there's nothing to do.
The **AppImage bundles** those LGPL libraries. That's allowed (dynamic linking, user can replace
the `.so` files by extracting the AppImage), but the release must make the corresponding source
available, e.g. by stating the exact Ubuntu 22.04 package versions (Ubuntu's source packages
satisfy the "written offer" in practice) and including their license texts.
linuxdeploy copies `copyright` files where available. Check this before the first public AppImage.

## External programs we integrate with

Yomika **starts these as separate processes** (`CommandSpec` argv + env, no shell) and
talks to them through command-line arguments, environment variables, files, pipes or HTTP. It
doesn't link, vendor or ship any of them.

| Program | License (verified 2026-10) | How we use it |
|---|---|---|
| Wine | LGPL-2.1-or-later | Run Windows games (`wine`, `wineserver`) |
| Proton (Valve) | BSD-3-Clause for the top-level project (`LICENSE.proton`), with each bundled component under its own license (Wine LGPL-2.1+, DXVK Zlib, vkd3d-proton LGPL-2.1, ...) | Runner via umu-launcher |
| GE-Proton | Same mix as Proton (no single SPDX license) | Runner |
| umu-launcher | GPL-3.0 | `umu-run` executes Proton in the Steam Runtime |
| winetricks | LGPL-2.1-or-later | Install runtimes into prefixes |
| gamescope | BSD-2-Clause (plus third-party components, e.g. OpenColorIO LUT code, under their own permissive licenses) | Nested compositor wrapper |
| LunaTranslator | GPL-3.0 | Reference / optional external tool |
| LunaHook | GPL-3.0 (now developed inside the LunaTranslator repo, `src/NativeImpl/LunaHook`; the standalone `HIllya51/LunaHook` repo no longer exists; forks such as `ZUIcat/LunaHook` are GPL-3.0) | Planned hook backend, run as a separate host process inside the game's prefix |
| Textractor | GPL-3.0 | Possible alternative hook backend |
| linux-rt-upscaler (`baronsmv/linux-rt-upscaler`) | GPL-3.0 | Optional real-time upscaler companion process |
| Tesseract | Apache-2.0 | OCR (`tesseract` CLI) |
| Ollama | MIT | Local translation/vision over HTTP |
| ImageMagick | ImageMagick License (Apache-2.0-style, permissive) | OCR preprocessing |
| wl-clipboard | GPL-3.0 | Clipboard (`wl-paste`) |
| DXVK | Zlib | Indirectly, via Wine/Proton prefixes |

### Implications

- **Separate programs, arm's-length communication.** Running a GPL program with `exec`,
  passing arguments and reading its output/pipes is, by the FSF's own GPL FAQ ("aggregate"
  vs. "combined work"), communication between separate programs. The GPL of umu-launcher,
  LunaHook, wl-clipboard or linux-rt-upscaler doesn't extend to Yomika, whatever license
  we pick. This holds as long as we:
  - don't copy their source code (including hook-code tables, Python modules or C++ helpers
    from LunaTranslator/LunaHook) into this repository,
  - don't link against their libraries (e.g. loading `LunaHost.dll`'s C API into *our*
    process with a tightly coupled, private interface would be the grey zone). The design runs
    the hook host as its own process inside Wine and talks to it over a pipe/socket, which is
    the safe pattern,
  - don't design them as a single program split across processes just to avoid the GPL.
- **DLL injection.** LunaHook injects its DLL into the **game's** process, not ours. That's
  between LunaHook and the game. Our code never loads the DLL.
- **If we ever bundle a GPL tool** (e.g. ship LunaHook binaries in the Flatpak or AppImage for
  convenience): that's "mere aggregation", which is allowed with any license for our code, but
  we must then comply with the GPL **for that binary**: include its license text, provide (or
  offer) the exact corresponding source, and not restrict users' rights to it. Proton/Wine
  (LGPL) bundling has the same source obligations. Simplest policy: **don't bundle; detect and
  invoke the user's installed copy** (the current design).
- **Trademarks/naming**: "Proton" and "Steam" are Valve trademarks. Describe compatibility
  ("runs Proton via umu-launcher"), don't brand the app with them.
- **Data**: VNDB metadata comes from the VNDB API. Check VNDB's current data license and API
  terms (the database has been published under the Open Database License; verify before
  redistributing any dumps). Cover images are the publishers' copyrighted works: cache them
  locally, don't redistribute them in packages or screenshots without permission.

## Project license: options (not decided)

The workspace uses `license = "LicenseRef-TBD"`. **Until a license is chosen, nobody else
has the right to use, modify or redistribute the code**, and external contributions are
legally murky. Decide before accepting outside contributions or publishing packages
(AUR/Flathub require an OSI/FSF license in practice).

| Option | What it allows | Pros | Cons |
|---|---|---|---|
| **MIT OR Apache-2.0** (Rust ecosystem default) | Anything, including proprietary forks, with attribution | Maximum reuse. Matches nearly all our dependencies. Apache-2.0 adds an explicit patent grant | Others can ship closed forks. Can't absorb GPL code (e.g. LunaHook sources) into this repo later |
| **MIT** only | Same as above, shorter | Simplest | No patent grant |
| **MPL-2.0** | File-level copyleft: modified *files* stay open, larger works can be any license | Middle ground. Compatible with GPL and permissive deps | Less familiar to contributors. Weaker than GPL against closed forks (new files can be closed) |
| **GPL-3.0-or-later** | Copyleft for the whole program: distributed derivatives must be GPL with source | Keeps forks open. Matches LunaTranslator, umu-launcher, linux-rt-upscaler. Would allow vendoring/porting GPL-3.0 code (hook logic, H-code databases) directly | Downstream reuse of our crates (e.g. `vnl-runtime` in a permissive project) becomes impossible. Apache-2.0 deps are fine (one-way compatible), GPL-2.0-only code would not be |
| **AGPL-3.0** | GPL + network-use clause | — | Pointless for a desktop app; deters contributors |
| Split licensing (crates permissive, app GPL) | Libraries `MIT OR Apache-2.0`, `src-tauri` + UI GPL-3.0 | Reusable building blocks plus a copyleft app | Two licenses to explain. Moving code between them needs care |

Things that change the answer:

1. **Do you want to integrate GPL code in-tree** (port LunaHook/LunaTranslator logic rather
   than call it)? If yes, GPL-3.0(-or-later) is effectively required.
2. **Do you want others to reuse the crates** (`vnl-runtime`'s runner/prefix handling,
   `vnl-graphics`'s gamescope builder) in permissive projects? That favours MIT/Apache.
3. **Do you care about closed forks/redistribution** (e.g. someone selling a re-skinned
   build)? That favours GPL or MPL.

Whatever is chosen, the mechanical follow-up is the same: add `LICENSE` (or `LICENSE-MIT` +
`LICENSE-APACHE`), set `license` in `[workspace.package]`, update
`packaging/arch/PKGBUILD` `license=()`, the metainfo `<project_license>`, and remove the
`LicenseRef-TBD` exceptions from `deny.toml`. Our own deny allow-list doesn't need to change
for any option. With GPL-3.0, every dependency must be GPL-3.0-compatible. All current ones
appear to be (MIT, Apache-2.0, BSD, ISC, Zlib, MPL-2.0, Unicode-3.0, CDLA-Permissive-2.0).

## Generated dependency report

<!-- Generated by scripts/license-report.py; do not edit by hand. -->

### Rust crates: shipped in the app (419)

| License (SPDX expression) | Packages | Class |
|---|---:|---|
| `MIT OR Apache-2.0` | 184 | permissive |
| `MIT` | 111 | permissive |
| `Apache-2.0 OR MIT` | 48 | permissive |
| `Unicode-3.0` | 18 | permissive |
| `MIT/Apache-2.0` | 14 | permissive |
| `MPL-2.0` | 5 | weak-copyleft |
| `BSD-3-Clause` | 3 | permissive |
| `ISC` | 3 | permissive |
| `Unlicense OR MIT` | 3 | permissive |
| `(MIT OR Apache-2.0) AND Unicode-3.0` | 2 | permissive |
| `Apache-2.0` | 2 | permissive |
| `Apache-2.0 OR ISC OR MIT` | 2 | permissive |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 2 | permissive |
| `Apache-2.0/MIT` | 2 | permissive |
| `MIT OR Apache-2.0 OR Zlib` | 2 | permissive |
| `MIT OR Zlib OR Apache-2.0` | 2 | permissive |
| `Unlicense/MIT` | 2 | permissive |
| `Zlib` | 2 | permissive |
| `0BSD OR MIT OR Apache-2.0` | 1 | permissive |
| `Apache-2.0 / MIT` | 1 | permissive |
| `Apache-2.0 AND ISC` | 1 | permissive |
| `Apache-2.0 AND MIT` | 1 | permissive |
| `Apache-2.0 OR BSL-1.0` | 1 | permissive |
| `Apache-2.0 WITH LLVM-exception` | 1 | permissive |
| `BSD-2-Clause OR Apache-2.0 OR MIT` | 1 | permissive |
| `BSD-3-Clause AND MIT` | 1 | permissive |
| `BSD-3-Clause/MIT` | 1 | permissive |
| `CC0-1.0 OR MIT-0 OR Apache-2.0` | 1 | permissive |
| `CDLA-Permissive-2.0` | 1 | permissive |
| `Zlib OR Apache-2.0 OR MIT` | 1 | permissive |

Non-permissive or unknown (needs review):

| Package | Version | License | Class |
|---|---|---|---|
| cssparser | 0.36.0 | `MPL-2.0` | weak-copyleft |
| cssparser-macros | 0.6.1 | `MPL-2.0` | weak-copyleft |
| dtoa-short | 0.3.5 | `MPL-2.0` | weak-copyleft |
| option-ext | 0.2.0 | `MPL-2.0` | weak-copyleft |
| selectors | 0.36.1 | `MPL-2.0` | weak-copyleft |

<details><summary>Rust crates: dev/build-only (7)</summary>

| License (SPDX expression) | Packages | Class |
|---|---:|---|
| `MIT OR Apache-2.0` | 5 | permissive |
| `MIT` | 1 | permissive |
| `MIT/Apache-2.0` | 1 | permissive |

</details>

### npm packages: shipped in the app (13)

| License (SPDX expression) | Packages | Class |
|---|---:|---|
| `MIT` | 9 | permissive |
| `MIT OR Apache-2.0` | 2 | permissive |
| `Apache-2.0 OR MIT` | 1 | permissive |
| `ISC` | 1 | permissive |

Non-permissive or unknown (needs review):

_None._

<details><summary>npm packages: dev/build-only (264)</summary>

| License (SPDX expression) | Packages | Class |
|---|---:|---|
| `MIT` | 192 | permissive |
| `Apache-2.0` | 18 | permissive |
| `Apache-2.0 OR MIT` | 12 | permissive |
| `MPL-2.0` | 12 | weak-copyleft |
| `ISC` | 11 | permissive |
| `BSD-2-Clause` | 8 | permissive |
| `BlueOak-1.0.0` | 4 | permissive |
| `BSD-3-Clause` | 3 | permissive |
| `MIT-0` | 2 | permissive |
| `CC-BY-4.0` | 1 | permissive |
| `CC0-1.0` | 1 | permissive |

| Package | Version | License | Class |
|---|---|---|---|
| lightningcss | 1.33.0 | `MPL-2.0` | weak-copyleft |
| lightningcss-android-arm64 | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-darwin-arm64 | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-darwin-x64 | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-freebsd-x64 | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-linux-arm-gnueabihf | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-linux-arm64-gnu | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-linux-arm64-musl | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-linux-x64-gnu | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-linux-x64-musl | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-win32-arm64-msvc | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |
| lightningcss-win32-x64-msvc | 1.33.0 | `MPL-2.0` | weak-copyleft (optional/platform-specific) |

</details>

