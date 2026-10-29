#!/usr/bin/env python3
"""Tauri config override for RPM's separate Version and Release fields."""
import json
from pathlib import Path
import re


def rpm_config(version: str) -> dict:
    match = re.fullmatch(
        r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
        r"(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?",
        version,
    )
    if not match:
        raise ValueError(f"Invalid application version: {version!r}")
    base = ".".join(match.group(i) for i in (1, 2, 3))
    prerelease = match.group(4)
    # Release 0 sorts before the final package's default Release 1.
    release = "0." + prerelease.replace("-", ".") if prerelease else "1"
    return {"version": base, "bundle": {"linux": {"rpm": {"release": release}}}}


if __name__ == "__main__":
    config = Path(__file__).resolve().parent.parent / "src-tauri/tauri.conf.json"
    print(json.dumps(rpm_config(json.loads(config.read_text())["version"])))
