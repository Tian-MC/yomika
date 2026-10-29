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
