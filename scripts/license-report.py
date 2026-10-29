#!/usr/bin/env python3
"""Summarise third-party licenses of Rust and npm dependencies.

Rust: `cargo metadata --filter-platform <linux triple>`; only crates reachable from
the workspace through normal/build edges are counted (dev-dependencies are listed
separately because they never ship).
npm: `package-lock.json` (lockfile v3 records each package's license and whether it
is dev-only).

Usage:
  scripts/license-report.py                # Markdown report on stdout
  scripts/license-report.py --check        # exit 1 if a shipped dep is strong-copyleft/unknown
                                           # (weak copyleft such as MPL-2.0 only warns)
  scripts/license-report.py --json out.json

Classification is a heuristic over SPDX expressions: an `OR` expression is as
permissive as its most permissive option; `AND` is as restrictive as its most
restrictive part.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TRIPLE = "x86_64-unknown-linux-gnu"

PERMISSIVE = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause",
    "BSD-1-Clause", "0BSD", "ISC", "Zlib", "Unlicense", "CC0-1.0", "BSL-1.0", "Unicode-3.0",
    "Unicode-DFS-2016", "BlueOak-1.0.0", "Python-2.0", "CC-BY-4.0", "WTFPL", "NCSA", "X11",
    "OpenSSL", "bzip2-1.0.6", "CDLA-Permissive-2.0", "Artistic-2.0",
}
WEAK_COPYLEFT = {"MPL-2.0", "LGPL-2.1", "LGPL-2.1-only", "LGPL-2.1-or-later", "LGPL-3.0", "LGPL-3.0-only",
                 "LGPL-3.0-or-later", "EPL-2.0", "CDDL-1.0", "CC-BY-SA-4.0"}
STRONG_COPYLEFT = {"GPL-2.0", "GPL-2.0-only", "GPL-2.0-or-later", "GPL-3.0", "GPL-3.0-only",
                   "GPL-3.0-or-later", "AGPL-3.0", "AGPL-3.0-only", "AGPL-3.0-or-later"}
RANK = {"permissive": 0, "weak-copyleft": 1, "strong-copyleft": 2, "unknown": 3}


def classify_atom(atom: str) -> str:
    atom = atom.strip().strip("()").strip()
    if atom.endswith("+"):
        atom = atom[:-1] + "-or-later"
    if atom in PERMISSIVE:
        return "permissive"
    if atom in WEAK_COPYLEFT:
        return "weak-copyleft"
    if atom in STRONG_COPYLEFT:
        return "strong-copyleft"
    return "unknown"


def classify(expr: str | None) -> str:
    """Very small SPDX evaluator: handles AND/OR/WITH and parentheses (flattened)."""
    if not expr:
        return "unknown"
    expr = expr.replace("/", " OR ")  # legacy cargo syntax "MIT/Apache-2.0"
    # Keep "X WITH exception" atoms together.
    expr = re.sub(r"\s+WITH\s+", "_WITH_", expr)
    # Split on top-level OR first: best option wins.
    or_parts = re.split(r"\s+OR\s+", expr.replace("(", " ").replace(")", " "))
    best = 99
    for part in or_parts:
        worst = 0
        for atom in re.split(r"\s+AND\s+", part.strip()):
            atom = atom.replace("_WITH_", " WITH ").strip()
            c = classify_atom(atom)
            if c == "unknown" and " WITH " in atom:
                c = classify_atom(atom.split(" WITH ")[0])
            worst = max(worst, RANK[c])
        best = min(best, worst)
    return {v: k for k, v in RANK.items()}[best]


def cargo_bin() -> str:
    found = shutil.which("cargo")
    if found:
        return found
    candidate = Path(os.environ.get("CARGO_HOME", Path.home() / ".cargo")) / "bin" / "cargo"
    return str(candidate)


def rust_deps() -> dict:
    out = subprocess.run(
        [cargo_bin(), "metadata", "--format-version", "1", "--filter-platform", TRIPLE],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    meta = json.loads(out)
    pkgs = {p["id"]: p for p in meta["packages"]}
    members = set(meta["workspace_members"])
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}

    def walk(start_ids, allow_dev_from_members: bool) -> set[str]:
        seen: set[str] = set()
        stack = list(start_ids)
        while stack:
            nid = stack.pop()
            if nid in seen:
                continue
            seen.add(nid)
            for dep in nodes[nid]["deps"]:
                kinds = {k["kind"] for k in dep["dep_kinds"]}
                shipped = kinds & {None, "build"}
                if shipped or (allow_dev_from_members and nid in members and "dev" in kinds):
                    stack.append(dep["pkg"])
        return seen

    shipped = walk(members, False) - members
    with_dev = walk(members, True) - members
    dev_only = with_dev - shipped
    # Anything reachable from a dev-only crate is dev-only too.
    dev_only |= walk(dev_only, False) - shipped - members

    def row(pid):
        p = pkgs[pid]
        lic = p.get("license") or (f"file: {p['license_file']}" if p.get("license_file") else None)
        return {"name": p["name"], "version": p["version"], "license": lic, "class": classify(p.get("license"))}

    return {
        "shipped": sorted((row(i) for i in shipped), key=lambda r: r["name"]),
        "dev": sorted((row(i) for i in dev_only), key=lambda r: r["name"]),
        "workspace": sorted(f"{pkgs[i]['name']} ({pkgs[i].get('license')})" for i in members),
    }


def npm_deps() -> dict:
    lock = json.loads((ROOT / "package-lock.json").read_text())
    shipped, dev = [], []
    for path, info in lock.get("packages", {}).items():
        if not path:
            continue
        name = info.get("name") or path.split("node_modules/")[-1]
        lic = info.get("license")
        if isinstance(lic, dict):
            lic = lic.get("type")
        r = {"name": name, "version": info.get("version", "?"), "license": lic, "class": classify(lic),
             "optional": bool(info.get("optional"))}
        (dev if info.get("dev") else shipped).append(r)
    key = lambda r: (r["name"], r["version"])  # noqa: E731
    return {"shipped": sorted(shipped, key=key), "dev": sorted(dev, key=key)}


def table_summary(rows) -> list[str]:
    c = Counter(r["license"] or "UNKNOWN" for r in rows)
    lines = ["| License (SPDX expression) | Packages | Class |", "|---|---:|---|"]
    for lic, n in sorted(c.items(), key=lambda kv: (-kv[1], kv[0])):
        lines.append(f"| `{lic}` | {n} | {classify(None if lic == 'UNKNOWN' else lic)} |")
    return lines


def flagged(rows) -> list[str]:
    out = []
    for r in rows:
        if r["class"] != "permissive":
            extra = " (optional/platform-specific)" if r.get("optional") else ""
            out.append(f"| {r['name']} | {r['version']} | `{r['license']}` | {r['class']}{extra} |")
    return out


def markdown(rust, npm) -> str:
    lines = ["<!-- Generated by scripts/license-report.py; do not edit by hand. -->", ""]
    for title, data in (("Rust crates", rust), ("npm packages", npm)):
        if data is None:
            lines += [f"### {title}", "", "_Not available (generation failed)._", ""]
            continue
        lines += [f"### {title}: shipped in the app ({len(data['shipped'])})", ""]
        lines += table_summary(data["shipped"]) + [""]
        flags = flagged(data["shipped"])
        lines += ["Non-permissive or unknown (needs review):", ""]
        if flags:
            lines += ["| Package | Version | License | Class |", "|---|---|---|---|"] + flags
        else:
            lines += ["_None._"]
        lines += ["", f"<details><summary>{title}: dev/build-only ({len(data['dev'])})</summary>", ""]
        lines += table_summary(data["dev"]) + [""]
        dflags = flagged(data["dev"])
        if dflags:
            lines += ["| Package | Version | License | Class |", "|---|---|---|---|"] + dflags + [""]
        lines += ["</details>", ""]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--check", action="store_true", help="fail on shipped strong-copyleft/unknown licenses")
    ap.add_argument("--json", metavar="FILE", help="also write raw data as JSON")
    args = ap.parse_args()

    try:
        rust = rust_deps()
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"warning: cargo metadata failed: {e}", file=sys.stderr)
        rust = None
    try:
        npm = npm_deps()
    except FileNotFoundError as e:
        print(f"warning: package-lock.json missing: {e}", file=sys.stderr)
        npm = None

    if args.json:
        Path(args.json).write_text(json.dumps({"rust": rust, "npm": npm}, indent=2))
    print(markdown(rust, npm))

    if args.check:
        bad, weak = defaultdict(list), defaultdict(list)
        for eco, data in (("rust", rust), ("npm", npm)):
            for r in (data or {}).get("shipped", []):
                if r.get("optional") or r["class"] == "permissive":
                    continue
                target = weak if r["class"] == "weak-copyleft" else bad
                target[eco].append(f"{r['name']}@{r['version']} ({r['license']})")
        for eco, items in weak.items():
            print(f"note: {eco}: {len(items)} shipped weak-copyleft: {', '.join(items)}", file=sys.stderr)
        if bad:
            for eco, items in bad.items():
                print(f"{eco}: {len(items)} shipped non-permissive/unknown: {', '.join(items)}", file=sys.stderr)
            return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
