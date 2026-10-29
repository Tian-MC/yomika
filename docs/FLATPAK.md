# Flatpak: design, limitations and security

Status: the manifest at `packaging/flatpak/io.github.vnlauncher.VNLauncher.yml` validates
against the flatpak-builder JSON schema, and the offline-source generators run on our lock
files. **It hasn't been built or run.** The development host has no `flatpak-builder`. Treat
everything below as the design to verify, not as tested behaviour.

## TL;DR

A game launcher that drives the host's Wine/Proton/umu/gamescope **can't be meaningfully
sandboxed**. The Flatpak runs the UI and library in the sandbox and runs every game process
on the host through `flatpak-spawn --host`, which needs `--talk-name=org.freedesktop.Flatpak`.
That permission lets the app run any command as you, outside the sandbox. Flathub accepts it
for launchers (the Lutris and Heroic Flatpaks use the same escape), but users and
reviewers should understand it gives no real isolation.

If you want sandboxing, the native packages (Arch, deb, rpm, AppImage) aren't worse in this
respect, and they're simpler.

## Runtime choice

| Item | Value | Why |
|---|---|---|
| Runtime | `org.gnome.Platform//51` | Ships `webkit2gtk-4.1` (WebKitGTK 2.54) + GTK 3 + libsoup 3, which is exactly what Tauri 2 / wry link against. Built on freedesktop-sdk 26.08. |
| SDK extensions | `rust-stable//26.08`, `node24//26.08` | Rust 1.98 and Node 24 LTS (both available on Flathub for 26.08). |

**Warning: WebKitGTK 4.1 is going away from the GNOME runtime.** gnome-build-meta `master`
removed `webkit2gtk-4.1` from the Flatpak SDK on 2026-09-28 (commit "Remove webkit2gtk-4.1
from flatpak SDK"). GNOME 51 still has it (`elements/sdk-platform.bst` on branch `gnome-51`),
so GNOME 52 (expected March 2027) won't. Options then: build WebKitGTK 4.1 as a manifest
module (a 1-2 hour build, heavy for Flathub's builders), stay on GNOME 51 until its EOL
(runtimes get about one year of updates), or move once Tauri/wry supports WebKitGTK 6 / GTK 4.

## Process model

```
 sandbox (bwrap, own PID namespace)            host
 ┌────────────────────────────────────┐        ┌─────────────────────────────────────┐
 │ vn-launcher (Tauri, WebKitGTK)     │        │ flatpak-session-helper (D-Bus)      │
 │   └─ flatpak-spawn --host ... ─────┼─D-Bus─►│   └─ umu-run / wine / gamescope ... │
 │        (proxy process, our PID)    │        │        └─ game.exe, wineserver      │
 └────────────────────────────────────┘        └─────────────────────────────────────┘
```

`vnl-runtime` already wraps every `CommandSpec` with `flatpak-spawn --host --env=K=V ...
--directory=<cwd> <program> <args>` when `/.flatpak-info` exists
(`crates/vnl-runtime/src/command.rs: flatpak_host_command`). The details below matter for
that implementation.

### Environment

`flatpak-spawn --host` does **not** forward the sandbox environment. The host process gets
the environment of `flatpak-session-helper` (the user's systemd/D-Bus activation environment)
plus whatever is passed with `--env=`. Consequences:

- Every variable the game needs (`WINEPREFIX`, `LANG`/`LC_ALL`, `PROTONPATH`, `GAMEID`,
  `WINEDLLOVERRIDES`, `WINE_FULLSCREEN_FSR`, ...) must be in `CommandSpec.env` explicitly.
  The current implementation does this.
- `DISPLAY`, `WAYLAND_DISPLAY` and `XDG_RUNTIME_DIR` are normally in the activation environment
  on GNOME/KDE. On minimal window-manager setups they may be missing
  (`dbus-update-activation-environment --systemd DISPLAY WAYLAND_DISPLAY` fixes it). Consider
  forwarding them explicitly. Note that inside the sandbox `WAYLAND_DISPLAY` may be a
  sandbox-specific socket name, so read the real values from the host
  (`flatpak-spawn --host printenv WAYLAND_DISPLAY`) rather than copying the sandbox's.
- `PATH` is the host's. Use program names (`umu-run`, `wine`) or host paths, never `/app/...`.

### Paths

- `/app`, `/usr` (runtime) and `/run/user/<uid>/doc/...` (document portal) exist **only inside
  the sandbox**. A host process can't see them. So:
  - **Don't use portal-provided paths for game folders.** The Tauri file dialog in a Flatpak
    goes through the FileChooser portal. With `--filesystem=home` it returns real paths for
    files under `$HOME`, but files outside granted locations come back as
    `/run/user/1000/doc/<id>/...`, which won't work for host Wine. Detect `/run/user/*/doc/`
    prefixes and refuse with a clear error, or translate them via the Documents portal's
    `GetHostPaths` method (recent xdg-desktop-portal versions).
  - Helpers shipped inside the Flatpak (e.g. a text-hook host `.exe`/DLLs bundled in
    `/app/share/...`) must be **copied to a host-visible directory** (e.g. the app data dir)
    before a host process can load them.
- `~/.var/app/io.github.vnlauncher.VNLauncher/` is bind-mounted at the same path, so data
  under the app's XDG dirs (prefixes, logs, runners) has the **same path on both sides**.
  That's why prefixes created by the launcher work for host Wine.
- `--directory=` (cwd) must be a host path that exists. A game folder under `$HOME` is fine.

### Filesystem permissions

The manifest grants `--filesystem=home` plus read-only Steam directories. Game libraries on
other drives (`/mnt/games`, `/run/media/...`) need extra grants:
`flatpak override --user --filesystem=/mnt/games io.github.vnlauncher.VNLauncher`.
Since the app can already run arbitrary host commands, narrower filesystem permissions add
little security. They only keep the *sandboxed* UI process honest.

Steam / Proton locations to read runners from:

| Steam install | compatibilitytools.d |
|---|---|
| Native | `~/.steam/steam/compatibilitytools.d`, `~/.local/share/Steam/compatibilitytools.d` |
| Flatpak Steam | `~/.var/app/com.valvesoftware.Steam/.local/share/Steam/compatibilitytools.d` (or `.../data/Steam/...` on older installs) |

Official Proton builds live in `<steam>/steamapps/common/Proton*`. Running them via host
`umu-run` with `PROTONPATH=<that dir>` works because the path is a real host path.

### PIDs, process groups and signals

The sandbox has its **own PID namespace**:

- The `Child` we spawn is `flatpak-spawn`, a proxy inside the sandbox. Host PIDs aren't
  visible in the sandbox's `/proc`, and `kill()`/`killpg()` from inside can't reach host
  processes.
- **`/proc` scans don't work.** `vnl-runtime`'s detection of leftover Wine processes by
  scanning `/proc/*/environ` for `WINEPREFIX` (`process::wine_user_processes`) sees nothing
  in a Flatpak. Without a replacement, a session would be considered "exited" as soon as the
  launcher stub exits, so playtime would be under-counted for games whose `.exe` re-execs.
  Recommended replacement: after the main process exits, run
  `flatpak-spawn --host --env=WINEPREFIX=<pfx> wineserver -w` (or `umu-run wineserver -w`
  for Proton) and treat its exit as the end of the session.
- **Signal forwarding** (from `flatpak-spawn.c`): `SIGHUP, SIGINT, SIGQUIT, SIGTERM,
  SIGCONT, SIGTSTP, SIGUSR1, SIGUSR2` sent to `flatpak-spawn` are forwarded through
  `HostCommandSignal`. Only `SIGINT`, `SIGSTOP`/`SIGTSTP` and `SIGCONT` go to the host
  **process group**. `SIGTERM` goes to the **direct child PID only**. `SIGKILL` can't be
  caught, so it is **never forwarded**: `SIGKILL`ing `flatpak-spawn` orphans the game on the
  host.
- Recommended stop sequence in Flatpak mode:
  1. `SIGINT` (forwarded to the host process group) or `SIGTERM` to the `flatpak-spawn` child.
  2. After the grace period: `flatpak-spawn --host --env=WINEPREFIX=<pfx> wineserver -k`
     (Proton: through `umu-run`/Proton's own wineserver, `WINESERVER` env), which kills
     every Wine process in the prefix.
  3. Only then `SIGKILL` the local `flatpak-spawn` proxy.
- Pass `--watch-bus` to `flatpak-spawn` so the host command is killed if the app's D-Bus
  connection goes away (app crash). Don't use it for detached auxiliary tools that should
  outlive the launcher.

### Wine inside the sandbox vs. on the host

| Approach | Pros | Cons |
|---|---|---|
| **Host Wine/Proton via `flatpak-spawn --host`** (chosen) | Uses the user's Wine/Proton/umu/gamescope, drivers and Steam compat tools as-is. Same behaviour as native packages. | Sandbox escape. All runtime deps must be on the host. PID/signal indirection. |
| Wine inside the sandbox (bundle Wine or use an extension, like Bottles/Lutris) | Self-contained, no host escape needed for games. | There's no generic `org.winehq.Wine` *extension* to depend on. WineHQ's Flatpak is an app, and Bottles/Lutris ship their own runners. We'd need to bundle Wine (hundreds of MB, 32-bit `org.freedesktop.Platform.Compat.i386` + `GL32` extensions), and umu/Proton start the Steam Runtime (pressure-vessel), which can't nest bwrap unprivileged. The Steam Flatpak works around that with the Flatpak portal's sub-sandboxes (`org.freedesktop.portal.Flatpak`), which is complex and limited. Host gamescope can't be used. |

Running Proton through umu itself launches a pressure-vessel (bwrap) container. Nesting that
inside the Flatpak sandbox needs the portal sub-sandbox machinery mentioned above, which is
the main reason to execute on the host.

### Text hooking and DLL injection

Hook-based text extraction (LunaHook-style) injects a DLL into the game's process **inside
Wine**. In the Flatpak:

- The hook host must run in the **same prefix and wineserver** as the game, so it must be
  spawned on the host too (`flatpak-spawn --host` + same `WINEPREFIX`/runner, `PROTON_VERB=runinprefix`
  for Proton, as `build_prefix_command` already does).
- The hook binaries/DLLs must sit at a host-visible path (copy from `/app` to the data dir).
- IPC between the hook host and the launcher: pipes from `flatpak-spawn` stdout/stderr work
  (they're forwarded). Unix sockets or shared memory in `/tmp` **don't**, because the sandbox
  has a private `/tmp`. Use a socket under `$XDG_RUNTIME_DIR/app/$FLATPAK_ID/` (shared with the
  host at the same path) or stdout.
- Nothing about injection gets more dangerous in Flatpak mode, since the hook already runs
  with full user rights on the host.

### gamescope

- gamescope runs on the host (`flatpak-spawn --host gamescope ... -- umu-run game.exe`) as a
  nested compositor in the user's session. Use the host gamescope binary. Flathub's
  `org.freedesktop.Platform.VulkanLayer.gamescope` extension is meant for apps that run games
  *inside* their sandbox (Steam Flatpak) and doesn't apply here.
- gamescope needs the host's Wayland/X11 sockets and `/dev/dri`. As a host process it has
  them. The sandbox's own `--device=dri` is only for WebKitGTK GPU compositing.
- The gamescope WSI layer and `ENABLE_GAMESCOPE_WSI` work as on a native install, because
  they're host-side.

### Display sockets, GPU, audio

- `--socket=wayland` + `--socket=fallback-x11` + `--share=ipc` + `--device=dri` are for the
  **UI** (WebKitGTK). Games don't use the sandbox's sockets.
- No `--socket=pulseaudio` is needed, because games play audio from the host. Add it only if
  the UI plays sound.
- `--share=network` is needed for VNDB, online translation and a local Ollama
  (`127.0.0.1:11434` is reachable since the sandbox shares the host network namespace when
  network is granted).

### Detecting host dependencies

Dependency checks (`vnl-graphics` `deps.rs`, `which`-style lookups) must run on the host too
in Flatpak mode. Following the "never use a shell" rule, call e.g.
`flatpak-spawn --host which umu-run` or `flatpak-spawn --host test -x <path>`. In-sandbox `which` only sees the GNOME runtime.
Locale checks (`locale -a`) and font checks (`fc-list`) must also run on the host, since the
sandbox has its own locales and fonts.

## Security implications (for the README / Flathub review)

- `--talk-name=org.freedesktop.Flatpak` gives the app arbitrary code execution as the user on
  the host. Flathub shows the app as "potentially unsafe" / "can run commands on the host".
  This is inherent to driving host Wine/Proton and is documented here so users can choose.
- `--filesystem=home` is broad but, given the above, adds no additional capability.
- Any vulnerability in the webview (e.g. XSS via VNDB metadata rendered as HTML) is
  effectively a host compromise. Keep the strict CSP in `tauri.conf.json`, never render remote
  HTML, and keep the Tauri command surface narrow (commands accept IDs, not raw argv).
- Mitigations: build `CommandSpec`s only from validated settings (no shell, already a project
  rule), confirm before running user-supplied executables the first time, and log every host
  command.

## Building

See `packaging/flatpak/README.md` for generating `cargo-sources.json` / `node-sources.json` and
for the `flatpak-builder` invocation (`make flatpak`).
