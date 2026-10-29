# Yomika developer entry points. Run `make help` for the list.
#
# rustup installs cargo into ~/.cargo/bin, which is often not on PATH for
# non-login shells (or fish users). We prepend it so cargo, rustc and the Tauri
# CLI (which shells out to cargo) all resolve without `source ~/.cargo/env`.

SHELL := /bin/bash
.SHELLFLAGS := -eu -o pipefail -c
.DEFAULT_GOAL := help
MAKEFLAGS += --no-print-directory

CARGO_HOME ?= $(HOME)/.cargo
export PATH := $(CARGO_HOME)/bin:$(PATH)

CARGO ?= cargo
NPM   ?= npm
TAURI := $(NPM) run tauri --

# Optional isolated data dir: `make dev VNL_HOME=$(PWD)/.vnl-dev` (or `make dev-isolated`).
VNL_HOME ?=
ifneq ($(strip $(VNL_HOME)),)
export VNL_HOME
endif

APP_ID      := io.github.yomika.Yomika
DIST_DIR    := $(CURDIR)/target/release/bundle
MAKEPKG_DIR := $(CURDIR)/target/makepkg
FLATPAK_DIR := $(CURDIR)/target/flatpak
FLATPAK_MANIFEST := packaging/flatpak/$(APP_ID).yml

.PHONY: help setup dev dev-isolated build test test-rust test-js lint fmt fmt-check clippy eslint typecheck \
        check ci audit bundle-appimage bundle-deb bundle-rpm bundles arch-pkg arch-srcinfo flatpak doctor clean-dev

help: ## Show this help
	@awk 'BEGIN {FS = ":.*## "} /^[a-zA-Z0-9_-]+:.*## / {printf "  \033[36m%-16s\033[0m %s\n", $$1, $$2}' $(MAKEFILE_LIST)

doctor: ## Check build/runtime dependencies and print install commands
	@scripts/dev-setup.sh

setup: ## Install JS deps (npm ci) and prefetch Rust crates
	@command -v $(CARGO) >/dev/null || { echo "cargo not found; install rustup (see docs/DEVELOPMENT.md)"; exit 1; }
	$(NPM) ci
	$(CARGO) fetch --locked

dev: ## Run the app with hot reload (set VNL_HOME=... for isolated data)
	@if [ -n "$${VNL_HOME:-}" ]; then echo "Using isolated data dir VNL_HOME=$$VNL_HOME"; mkdir -p "$$VNL_HOME"; fi
	$(TAURI) dev

dev-isolated: ## Run the app with data under ./.vnl-dev
	@$(MAKE) dev VNL_HOME=$(CURDIR)/.vnl-dev

build: ## Release build of the binary without installers (target/release/yomika)
	$(TAURI) build --no-bundle

test: test-rust test-js ## Run all tests

test-rust: ## cargo test --workspace
	$(CARGO) test --workspace --locked

test-js: ## vitest
	$(NPM) test

lint: fmt-check clippy eslint typecheck ## All static checks (no changes made)

fmt: ## Format Rust code in place
	$(CARGO) fmt --all

fmt-check: ## Verify Rust formatting
	$(CARGO) fmt --all -- --check

clippy: ## Clippy with warnings as errors
	$(CARGO) clippy --workspace --all-targets --locked -- -D warnings

eslint: ## ESLint
	$(NPM) run lint

typecheck: ## TypeScript type check
	$(NPM) run typecheck

check: ## Fast compile check (cargo check + tsc)
	$(CARGO) check --workspace --all-targets --locked
	$(NPM) run typecheck

ci: ## Full validation suite (same as CI)
	scripts/check-all.sh

audit: ## npm audit + cargo-deny (if installed)
	$(NPM) audit --omit=dev || true
	@if $(CARGO) deny --version >/dev/null 2>&1; then $(CARGO) deny check; \
	 else echo "cargo-deny not installed: cargo install --locked cargo-deny"; fi

bundle-appimage: ## Build an AppImage (Tauri downloads linuxdeploy itself)
	$(TAURI) build --bundles appimage
	@ls -1 $(DIST_DIR)/appimage/*.AppImage

bundle-deb: ## Build a .deb
	$(TAURI) build --bundles deb
	@ls -1 $(DIST_DIR)/deb/*.deb

bundle-rpm: ## Build an .rpm
	scripts/bundle-linux.sh rpm
	@ls -1 $(DIST_DIR)/rpm/*.rpm

bundles: ## Build AppImage + deb + rpm in one go
	scripts/bundle-linux.sh

# Builds the *committed* HEAD of this checkout (makepkg clones it via git+file://).
# Missing dependencies are reported, not installed; add MAKEPKG_FLAGS=-s to let makepkg use sudo pacman.
MAKEPKG_FLAGS ?=
arch-pkg: ## Build an Arch package from the committed HEAD (packaging/arch/PKGBUILD)
	@command -v makepkg >/dev/null || { echo "makepkg not found (Arch Linux only)"; exit 1; }
	mkdir -p $(MAKEPKG_DIR)/{src,build,out}
	cd packaging/arch && VNL_REPO="file://$(CURDIR)" SRCDEST=$(MAKEPKG_DIR)/src BUILDDIR=$(MAKEPKG_DIR)/build \
	  PKGDEST=$(MAKEPKG_DIR)/out makepkg -f --cleanbuild $(MAKEPKG_FLAGS)
	@ls -1 $(MAKEPKG_DIR)/out/*.pkg.tar.*

arch-srcinfo: ## Regenerate packaging/arch/.SRCINFO
	cd packaging/arch && makepkg --printsrcinfo > .SRCINFO

flatpak: ## Build + install the Flatpak locally (needs flatpak-builder and generated sources)
	@if command -v flatpak-builder >/dev/null; then FB=flatpak-builder; \
	 elif flatpak info org.flatpak.Builder >/dev/null 2>&1; then FB="flatpak run org.flatpak.Builder"; \
	 else echo "flatpak-builder not found: install it or 'flatpak install flathub org.flatpak.Builder'"; exit 1; fi; \
	 for f in cargo-sources.json node-sources.json; do \
	   [ -f packaging/flatpak/$$f ] || { echo "packaging/flatpak/$$f missing: see packaging/flatpak/README.md"; exit 1; }; done; \
	 $$FB --user --install-deps-from=flathub --force-clean --install --repo=$(FLATPAK_DIR)/repo \
	   --state-dir=$(FLATPAK_DIR)/state $(FLATPAK_DIR)/build $(FLATPAK_MANIFEST)

clean-dev: ## Remove the isolated dev data dir (.vnl-dev)
	rm -rf $(CURDIR)/.vnl-dev
