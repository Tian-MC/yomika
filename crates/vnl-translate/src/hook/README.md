# Legacy CLI text hooking

Install user-provided **LunaHook 3.x `LunaHostCLI32.exe` / `LunaHostCLI64.exe`**
with the corresponding `LunaHost32/64.dll` and `LunaHook32/64.dll`, or
**`TextractorCLI.exe`** with `texthook.dll`, under `<tools_dir>/lunahook/`.
Architecture subdirectories are supported. The CLI must match the game's PE
architecture. Current LunaTranslator's DLL-only C API is a separate integration
and is not supported by this adapter.

Enable extraction in the game's Translation tab, choose Hook (or Hook + OCR),
save settings and launch the game. No translation provider or target language
is needed to display original text. The existing prefix command builder runs
both `tasklist.exe` and the CLI with the game's runner and Wine/Proton prefix.
Only an unambiguous matching Windows executable name is accepted; Linux PIDs
and unrelated processes are never used as injection targets.

Advance dialogue, then choose **Text thread** in Live status. IDs and previews
refresh once a second. Until a thread is chosen, extraction only collects
previews. Choosing the paused entry stops forwarding. Only the selected runtime
thread reaches the existing normalization/deduplication pipeline; other threads
and other PIDs are ignored for translation. Queued events and late translation
responses from a previous selection are discarded.

The selection is saved immediately for that game in existing settings JSON.
Restoration matches name, reported hook code and both contexts, excluding PID,
handle and hook address. If an engine embeds changing addresses in its hook
code or contexts, choose the thread again; ambiguous identical fingerprints use
one runtime thread. Preview changes do not automatically select another thread.

Live diagnostics distinguish missing CLI, architecture/injection failures,
process discovery timeout, attach timeout and missing text. Closing the game or
launcher detaches and stops the host, with bounded waits and process-group
cleanup. The adapter also checks disappearance of the target Windows PID to
avoid keeping a Wine session alive just because its helper is still running.

Tests replay UTF-16 CLI output with temporary shell hosts, including cancellation
during startup, game exit, selection, deduplication and persistence. They do not
prove DLL injection on a real Wine/Proton visual novel. That needs a compatible
installed CLI and a real game; encrypted game resources need not be decrypted.

## GSM 1.0.2 x86 CLI crash on charset detection

The installed GSM 1.0.2 CLI can exit with code 9 at the first non-UTF-8
sentence. On Yorikure/BGI, Wine and a debugger confirm `c0000409`,
`FAST_FAIL_INVALID_ARG`, in the CLI's static CRT. The upstream charset detector
(`textthread.cpp`, `uchardetwrapper::detect`) calls narrow
`printf("%f %s %d\n", ...)` while the CLI has set stdout to `_O_U16TEXT`.
This debug print is incompatible with that stream mode. The source fix is to
remove that print (or send diagnostics to stderr).

For the exact GSM 1.0.2 `LunaHostCLI32.exe`, this repository provides an explicit,
one-time repair:

```sh
python3 scripts/repair_lunahost_cli.py /path/to/tools/lunahook/LunaHostCLI32.exe
```

Close the hook host before running it. It verifies the complete original SHA-256
and the instruction bytes, removes only the five-byte call to the debug print,
and atomically replaces the CLI after saving `.yomika-original`. No codepage
logic or PE section offsets change. Other binaries are rejected and existing
backups are never overwritten. Repeating the repair is harmless. To undo it,
restore the original backup; a later external engine reinstall may also restore
the affected original. The original package manifest is intentionally unchanged.
Yomika never applies this repair automatically.

The repaired CLI was tested on Yorikure with GE-Proton 11-11/umu: 15 Japanese
lines arrived from `EmbedBGI`, and detach exited with 0. This fixes the CLI
crash; it does not fix pressure-vessel replacing stdin with `/dev/null`.
`PRESSURE_VESSEL_DEVEL=1` was used only for the diagnostic run, and is not set
by Yomika or by the repair script.
