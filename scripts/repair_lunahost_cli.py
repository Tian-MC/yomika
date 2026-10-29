#!/usr/bin/env python3
"""Repair only GSM 1.0.2's x86 CLI debug printf crash; keep the original.

Upstream textthread.cpp's charset detector prints "%f %s %d" to stdout.
The CLI sets stdout to _O_U16TEXT, so this narrow printf triggers CRT
FAST_FAIL_INVALID_ARG (c0000409) when non-UTF-8 text reaches the detector.
The source fix is to remove that debug-only printf. For the exact known
binary below, NOP its call; its arguments are still popped by the caller.
No detector logic, protocol code, offsets or section sizes are changed.
"""

import argparse
import hashlib
import os
from pathlib import Path
import tempfile

ORIGINAL_SHA256 = "c769c20a79d8ab389c4a1182345dc7f412d3498be7df7d88464ec27e3b1a3271"
OFFSET = 0x18351
ORIGINAL_CALL = bytes.fromhex("e8eae2ffff")
REPLACEMENT = b"\x90" * 5
# Filled from the verified, same-length replacement of the known original.
PATCHED_SHA256 = "4517cfcd731953c29f43ba7c5285ebb036c51ce4ddc1cff9f110b96be23e5f77"


def digest(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def repair(executable: Path) -> bool:
    """Return True if repaired, False if already repaired; reject other builds."""
    if executable.is_symlink():
        raise ValueError("Pass the actual executable, not a symlink")
    original = executable.read_bytes()
    checksum = digest(original)
    if checksum == PATCHED_SHA256:
        return False
    if checksum != ORIGINAL_SHA256:
        raise ValueError(f"Unsupported CLI SHA-256 {checksum}; no files changed")
    if original[OFFSET : OFFSET + len(ORIGINAL_CALL)] != ORIGINAL_CALL:
        raise ValueError("Unexpected instruction at the verified call site; no files changed")
    patched = original[:OFFSET] + REPLACEMENT + original[OFFSET + len(ORIGINAL_CALL) :]
    if digest(patched) != PATCHED_SHA256:
        raise ValueError("Repaired checksum mismatch; no files changed")
    backup = executable.with_name(executable.name + ".yomika-original")
    try:
        with backup.open("xb") as stream:
            stream.write(original)
            stream.flush()
            os.fsync(stream.fileno())
    except FileExistsError:
        if backup.read_bytes() != original:
            raise ValueError(f"Backup {backup} differs; refusing to overwrite it") from None
    temporary = None
    try:
        with tempfile.NamedTemporaryFile(dir=executable.parent, delete=False) as stream:
            temporary = Path(stream.name)
            stream.write(patched)
            stream.flush()
            os.fchmod(stream.fileno(), executable.stat().st_mode & 0o777)
            os.fsync(stream.fileno())
        os.replace(temporary, executable)
    finally:
        if temporary is not None:
            temporary.unlink(missing_ok=True)
    return True


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("executable", type=Path, help="Path to the affected LunaHostCLI32.exe")
    args = parser.parse_args()
    try:
        changed = repair(args.executable)
    except (OSError, ValueError) as error:
        parser.exit(1, f"{error}\n")
    print("Repaired; original saved as .yomika-original" if changed else "Already repaired; no files changed")


if __name__ == "__main__":
    main()
