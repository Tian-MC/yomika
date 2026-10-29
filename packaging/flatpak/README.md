# Flatpak packaging

Manifest: `io.github.vnlauncher.VNLauncher.yml` (GNOME 51 runtime, rust-stable + node24 SDK
extensions, fully offline build). Read [`docs/FLATPAK.md`](../../docs/FLATPAK.md) first: the
app runs games on the host via `flatpak-spawn --host`, which is a sandbox escape.

> **Not built yet.** The manifest validates against flatpak-builder's JSON schema and both
> source generators below were run successfully on the current lock files (1111 cargo
> sources, 552 npm sources), but no `flatpak-builder` run has been done.

## 1. Tools

```sh
# flatpak-builder (either):
sudo pacman -S flatpak-builder            # or apt/dnf install flatpak-builder
flatpak install --user flathub org.flatpak.Builder   # then use: flatpak run org.flatpak.Builder

# runtimes
flatpak install --user flathub org.gnome.Platform//51 org.gnome.Sdk//51 \
  org.freedesktop.Sdk.Extension.rust-stable//26.08 org.freedesktop.Sdk.Extension.node24//26.08

# source generators
git clone https://github.com/flatpak/flatpak-builder-tools.git ~/src/flatpak-builder-tools
python3 -m venv ~/.venvs/fbt
~/.venvs/fbt/bin/pip install 'aiohttp>=3.9.5,<4' 'tomlkit>=0.13.3,<1' PyYAML \
  ~/src/flatpak-builder-tools/node          # installs flatpak-node-generator
```

## 2. Generate offline sources

Run from the repository root whenever `Cargo.lock` or `package-lock.json` changes:

```sh
~/.venvs/fbt/bin/python ~/src/flatpak-builder-tools/cargo/flatpak-cargo-generator.py \
  Cargo.lock -o packaging/flatpak/cargo-sources.json

~/.venvs/fbt/bin/flatpak-node-generator --no-requests-cache \
  npm package-lock.json -o packaging/flatpak/node-sources.json
```

- `cargo-sources.json` vendors every crate into `cargo/vendor` and writes `cargo/config`
  (source replacement). The manifest points `CARGO_HOME` at `/run/build/vn-launcher/cargo`.
- `node-sources.json` fills an npm cache in `flatpak-node/npm-cache` (the manifest's
  `npm_config_cache`) and adds `setup_sdk_node_headers.sh` (node-gyp headers in
  `flatpak-node/cache`, matched by `XDG_CACHE_HOME`). It includes the prebuilt native
  binaries of every platform for `@tauri-apps/cli`, rolldown and lightningcss, which is
  expected.
- Both files are large (~300 KB each) and generated, so commit them only for releases or
  Flathub submissions.

## 3. Build and run

```sh
make flatpak       # = flatpak-builder --user --install-deps-from=flathub --force-clean --install \
                   #     --repo=target/flatpak/repo --state-dir=target/flatpak/state \
                   #     target/flatpak/build packaging/flatpak/io.github.vnlauncher.VNLauncher.yml
flatpak run io.github.vnlauncher.VNLauncher
```

The manifest uses a `dir` source (`../..`, skipping `target/`, `node_modules/`, `dist/`), so
it builds your working tree. For Flathub, replace it with a `git` source pinned to a tag
**and** commit, and run the linter:

```sh
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest packaging/flatpak/io.github.vnlauncher.VNLauncher.yml
flatpak run --command=flatpak-builder-lint org.flatpak.Builder repo target/flatpak/repo
```

Expect the linter to flag `--talk-name=org.freedesktop.Flatpak` and `--filesystem=home`.
Both need a justification in the Flathub submission (see docs/FLATPAK.md).

## 4. Validating without flatpak-builder

```sh
python3 -m venv /tmp/fpv && /tmp/fpv/bin/pip install jsonschema pyyaml
curl -fsSLO https://raw.githubusercontent.com/flatpak/flatpak-builder/main/data/flatpak-manifest.schema.json
/tmp/fpv/bin/python - <<'EOF'
import json, yaml, jsonschema
schema = json.load(open("flatpak-manifest.schema.json"))
doc = yaml.safe_load(open("packaging/flatpak/io.github.vnlauncher.VNLauncher.yml"))
errors = list(jsonschema.Draft7Validator(schema).iter_errors(doc))
print(*[f"{list(e.path)}: {e.message}" for e in errors], sep="\n")
print("errors:", len(errors))
EOF
```
