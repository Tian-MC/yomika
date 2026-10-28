#!/usr/bin/env bash
# The `have X && ok ... || miss ...` idiom is safe here: ok()/opt() always return 0.
# shellcheck disable=SC2015
# Checks build and runtime dependencies for VN Launcher and PRINTS the commands
# needed to install whatever is missing. It never runs sudo or installs anything.
#
# Usage: scripts/dev-setup.sh [--build-only|--runtime-only] [--quiet]
# Exit status: 0 if all *required build* deps are present, 1 otherwise.
#
# Build prerequisites follow https://v2.tauri.app/start/prerequisites/ (Linux).

set -uo pipefail

MODE=all
QUIET=0
for arg in "$@"; do
  case "$arg" in
    --build-only) MODE=build ;;
    --runtime-only) MODE=runtime ;;
    --quiet | -q) QUIET=1 ;;
    -h | --help)
      sed -n '4,10p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

if [[ -t 1 ]]; then
  C_OK=$'\e[32m' C_WARN=$'\e[33m' C_ERR=$'\e[31m' C_DIM=$'\e[2m' C_B=$'\e[1m' C_0=$'\e[0m'
else
  C_OK='' C_WARN='' C_ERR='' C_DIM='' C_B='' C_0=''
fi

ok() { [[ $QUIET == 1 ]] || printf '  %s[ok]%s   %s\n' "$C_OK" "$C_0" "$*"; }
miss() { printf '  %s[miss]%s %s\n' "$C_ERR" "$C_0" "$*"; }
opt() { printf '  %s[opt]%s  %s\n' "$C_WARN" "$C_0" "$*"; }
hdr() { printf '\n%s%s%s\n' "$C_B" "$*" "$C_0"; }

# rustup installs into ~/.cargo/bin, which may not be on PATH (fish, non-login shells).
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
[[ -d $CARGO_BIN && :$PATH: != *":$CARGO_BIN:"* ]] && PATH="$CARGO_BIN:$PATH"

# ---------------------------------------------------------------- distro detection
DISTRO=unknown
if [[ -r /etc/os-release ]]; then
  # shellcheck disable=SC1091
  . /etc/os-release
  ids=" ${ID:-} ${ID_LIKE:-} "
  case "$ids" in
    *" arch "* | *" archlinux "* | *" manjaro "* | *" endeavouros "* | *" cachyos "*) DISTRO=arch ;;
    *" fedora "* | *" rhel "* | *" centos "* | *" nobara "*) DISTRO=fedora ;;
    *" opensuse"* | *" suse "* | *" sles "*) DISTRO=opensuse ;;
    *" debian "* | *" ubuntu "* | *" linuxmint "* | *" pop "*) DISTRO=debian ;;
  esac
fi
printf '%sDetected distro:%s %s (%s)\n' "$C_B" "$C_0" "$DISTRO" "${PRETTY_NAME:-unknown}"

# Per-distro package names, keyed by logical dependency.
# Format: key -> "arch|debian|fedora|opensuse" ("-" = not packaged / not applicable)
declare -A PKG=(
  [toolchain]="base-devel|build-essential|@c-development|-t pattern devel_basis"
  [pkgconfig]="pkgconf|pkg-config|pkgconf-pkg-config|pkg-config"
  [curl]="curl|curl|curl|curl"
  [wget]="wget|wget|wget|wget"
  [file]="file|file|file|file"
  [git]="git|git|git|git"
  [webkit]="webkit2gtk-4.1|libwebkit2gtk-4.1-dev|webkit2gtk4.1-devel|webkit2gtk3-devel"
  [gtk3]="gtk3|libgtk-3-dev|gtk3-devel|gtk3-devel"
  [soup3]="libsoup3|libsoup-3.0-dev|libsoup3-devel|libsoup-3_0-devel"
  [rsvg]="librsvg|librsvg2-dev|librsvg2-devel|librsvg-devel"
  [openssl]="openssl|libssl-dev|openssl-devel|libopenssl-devel"
  [xdo]="xdotool|libxdo-dev|libxdo-devel|xdotool-devel"
  [appindicator]="libayatana-appindicator|libayatana-appindicator3-dev|libappindicator-gtk3-devel|libappindicator3-1"
  [node]="nodejs npm|nodejs npm|nodejs npm|nodejs npm"
  # runtime (all optional except a Wine/Proton runner for Windows games)
  [wine]="wine|wine64 wine32:i386|wine|wine"
  [umu]="umu-launcher|-|umu-launcher|-"
  [winetricks]="winetricks|winetricks|winetricks|winetricks"
  [gamescope]="gamescope|gamescope|gamescope|gamescope"
  [tesseract]="tesseract tesseract-data-jpn|tesseract-ocr tesseract-ocr-jpn|tesseract tesseract-langpack-jpn|tesseract-ocr tesseract-ocr-traineddata-japanese"
  [ollama]="ollama|-|ollama|ollama"
  [imagemagick]="imagemagick|imagemagick|ImageMagick|ImageMagick"
  [wlclip]="wl-clipboard|wl-clipboard|wl-clipboard|wl-clipboard"
  [xclip]="xclip|xclip|xclip|xclip"
  [cjkfonts]="noto-fonts-cjk|fonts-noto-cjk|google-noto-sans-cjk-fonts|noto-sans-cjk-fonts"
  [vulkan32]="lib32-vulkan-icd-loader|libvulkan1:i386 mesa-vulkan-drivers:i386|vulkan-loader.i686 mesa-vulkan-drivers.i686|libvulkan1-32bit libvulkan_radeon-32bit"
)

pkg_for() {
  local spec="${PKG[$1]}" idx
  case "$DISTRO" in arch) idx=1 ;; debian) idx=2 ;; fedora) idx=3 ;; opensuse) idx=4 ;; *) idx=0 ;; esac
  [[ $idx == 0 ]] && {
    echo "?"
    return
  }
  cut -d'|' -f"$idx" <<<"$spec"
}

MISSING_BUILD=()
MISSING_BUILD_OPT=()
MISSING_RUNTIME=()
add_build() { MISSING_BUILD+=("$(pkg_for "$1")"); }
add_build_opt() { MISSING_BUILD_OPT+=("$(pkg_for "$1")"); }
add_runtime() {
  local p
  p="$(pkg_for "$1")"
  [[ $p != "-" ]] && MISSING_RUNTIME+=("$p")
}

have() { command -v "$1" >/dev/null 2>&1; }
pc() { have pkg-config && pkg-config --exists "$1" 2>/dev/null; }

version_ge() { # version_ge 20.19.0 20.0.0
  [[ "$(printf '%s\n%s\n' "$2" "$1" | sort -V | head -n1)" == "$2" ]]
}

# ---------------------------------------------------------------- build deps
check_build() {
  hdr "Build dependencies"

  if have rustup; then ok "rustup $(rustup --version 2>/dev/null | head -n1 | awk '{print $2}')"; fi
  if have cargo; then
    local rv need
    rv="$(rustc --version 2>/dev/null | awk '{print $2}')"
    need=1.85.0
    if version_ge "$rv" "$need"; then ok "rustc $rv (>= $need)"; else miss "rustc $rv is older than $need: run 'rustup update stable'"; fi
    for comp in rustfmt clippy; do
      if have "cargo-${comp/rustfmt/fmt}" || have "$comp"; then ok "$comp"; else
        miss "$comp: rustup component add $comp"
      fi
    done
  else
    miss "cargo/rustc: install rustup (distro rust packages also work if new enough):"
    echo "         curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    MISSING_BUILD+=("")
  fi

  if have node; then
    local nv
    nv="$(node --version | tr -d v)"
    # Vite 8 needs Node 20.19+ / 22.12+.
    if version_ge "$nv" 20.19.0; then ok "node $nv"; else
      miss "node $nv is too old (need >= 20.19)"
      add_build node
    fi
    have npm && ok "npm $(npm --version)" || add_build node
  else
    miss "node/npm"
    add_build node
  fi

  have cc || have gcc || have clang && ok "C compiler" || {
    miss "C compiler / make"
    add_build toolchain
  }
  have make && ok "make" || add_build toolchain
  have pkg-config && ok "pkg-config" || {
    miss "pkg-config"
    add_build pkgconfig
  }
  for t in curl wget file git; do
    have "$t" && ok "$t" || {
      miss "$t"
      add_build "$t"
    }
  done

  # pkg-config modules (headers + libs) needed to compile the Tauri shell.
  local mods=(
    "webkit2gtk-4.1:webkit"
    "javascriptcoregtk-4.1:webkit"
    "gtk+-3.0:gtk3"
    "libsoup-3.0:soup3"
    "librsvg-2.0:rsvg"
    "openssl:openssl"
  )
  local m mod key
  for m in "${mods[@]}"; do
    mod="${m%%:*}" key="${m##*:}"
    if pc "$mod"; then ok "$mod $(pkg-config --modversion "$mod")"; else
      miss "$mod (pkg-config)"
      add_build "$key"
    fi
  done
  # libxdo is listed by the Tauri prerequisites (global shortcuts / menu accelerators on X11).
  if [[ -e /usr/include/xdo.h ]] || ldconfig -p 2>/dev/null | grep -q libxdo; then ok "libxdo"; else
    opt "libxdo (listed in Tauri prerequisites; only needed by some plugins)"
    add_build_opt xdo
  fi
  # Tray icons are not used yet; appindicator is only needed if a tray feature is added.
  if pc ayatana-appindicator3-0.1 || pc appindicator3-0.1; then ok "appindicator"; else
    opt "libayatana-appindicator (only needed for a tray icon, currently unused)"
  fi
}

# ---------------------------------------------------------------- runtime deps
check_runtime() {
  hdr "Runtime dependencies (optional features)"

  local runners=0
  if have wine; then
    ok "wine $(wine --version 2>/dev/null)"
    runners=1
  else opt "wine: system Wine runner"; fi
  if have umu-run; then
    ok "umu-launcher ($(command -v umu-run))"
    runners=1
  else
    opt "umu-launcher: run Proton outside Steam (recommended)"
    add_runtime umu
    [[ $DISTRO == debian || $DISTRO == opensuse ]] &&
      echo "         umu-launcher: see https://github.com/Open-Wine-Components/umu-launcher/releases (deb/zipapp)"
  fi
  local steam_root seen_roots=" "
  for steam_root in "$HOME/.steam/steam" "$HOME/.local/share/Steam" "$HOME/.var/app/com.valvesoftware.Steam/.local/share/Steam"; do
    [[ -d $steam_root ]] || continue
    steam_root="$(realpath "$steam_root")"
    [[ $seen_roots == *" $steam_root "* ]] && continue
    seen_roots+="$steam_root "
    if compgen -G "$steam_root/steamapps/common/Proton*" >/dev/null || compgen -G "$steam_root/compatibilitytools.d/*" >/dev/null; then
      ok "Proton builds found under $steam_root"
      runners=1
    fi
  done
  if [[ $runners == 0 ]]; then
    miss "no Wine/Proton runner found: Windows games cannot be launched"
    add_runtime wine
  fi

  have winetricks && ok "winetricks" || {
    opt "winetricks: install runtimes (vcrun, d3dx9, fonts) into prefixes"
    add_runtime winetricks
  }
  have gamescope && ok "gamescope" || {
    opt "gamescope: nested compositor, FSR/NIS upscaling, fixed resolution"
    add_runtime gamescope
  }
  if have tesseract; then
    if tesseract --list-langs 2>/dev/null | grep -qx jpn; then ok "tesseract + jpn"; else
      opt "tesseract is installed but the 'jpn' language data is missing"
      add_runtime tesseract
    fi
  else
    opt "tesseract (+jpn data): OCR text source"
    add_runtime tesseract
  fi
  have ollama && ok "ollama" || {
    opt "ollama: local LLM translation (https://ollama.com/download)"
    add_runtime ollama
  }
  have magick || have convert && ok "ImageMagick" || {
    opt "ImageMagick: screenshot cropping/preprocessing for OCR"
    add_runtime imagemagick
  }
  if [[ ${XDG_SESSION_TYPE:-} == wayland ]] || [[ -n ${WAYLAND_DISPLAY:-} ]]; then
    have wl-copy && ok "wl-clipboard" || {
      opt "wl-clipboard: clipboard text source on Wayland"
      add_runtime wlclip
    }
  else
    have xclip && ok "xclip" || {
      opt "xclip: clipboard text source on X11"
      add_runtime xclip
    }
  fi

  # Japanese locale for games that need LANG=ja_JP.UTF-8 (see docs/TROUBLESHOOTING.md).
  if locale -a 2>/dev/null | grep -qiE '^ja_JP\.utf-?8$'; then ok "locale ja_JP.UTF-8"; else
    opt "locale ja_JP.UTF-8 not generated (needed by many Japanese games)"
    case "$DISTRO" in
      arch) echo "         sudo sed -i 's/^#ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen" ;;
      debian) echo "         sudo sed -i 's/^# *ja_JP.UTF-8/ja_JP.UTF-8/' /etc/locale.gen && sudo locale-gen   (Ubuntu: sudo locale-gen ja_JP.UTF-8)" ;;
      fedora) echo "         sudo dnf install glibc-langpack-ja" ;;
      opensuse) echo "         sudo zypper install glibc-locale" ;;
    esac
  fi
  if have fc-list && [[ -n "$(fc-list :lang=ja family 2>/dev/null | head -n1)" ]]; then ok "CJK fonts"; else
    opt "no Japanese fonts found (UI and host-side text rendering)"
    add_runtime cjkfonts
  fi
  if have vulkaninfo; then
    vulkaninfo --summary >/dev/null 2>&1 && ok "Vulkan (64-bit)" || opt "vulkaninfo failed: check GPU drivers"
  fi
  if [[ $DISTRO == arch ]] && ! pacman -Q lib32-vulkan-icd-loader >/dev/null 2>&1; then
    opt "32-bit Vulkan loader missing (needed for 32-bit games with DXVK); also install your GPU's lib32 driver"
    add_runtime vulkan32
  fi
}

[[ $MODE != runtime ]] && check_build
[[ $MODE != build ]] && check_runtime

# ---------------------------------------------------------------- install hints
print_cmd() { # title, packages...
  local title="$1"
  shift
  local pkgs=()
  local p
  local words
  for p in "$@"; do
    [[ -n $p && $p != "-" && $p != "?" ]] || continue
    read -ra words <<<"$p" # entries may hold several space-separated packages
    pkgs+=("${words[@]}")
  done
  ((${#pkgs[@]} == 0)) && return
  # de-duplicate while preserving order
  mapfile -t pkgs < <(printf '%s\n' "${pkgs[@]}" | awk '!seen[$0]++')
  hdr "$title"
  case "$DISTRO" in
    arch) echo "  sudo pacman -S --needed ${pkgs[*]}" ;;
    debian) echo "  sudo apt update && sudo apt install ${pkgs[*]}" ;;
    fedora)
      local grp=() rest=()
      for p in "${pkgs[@]}"; do [[ $p == @* ]] && grp+=("${p#@}") || rest+=("$p"); done
      ((${#rest[@]})) && echo "  sudo dnf install ${rest[*]}"
      ((${#grp[@]})) && echo "  sudo dnf group install ${grp[*]}"
      ;;
    opensuse)
      local pat=() rest=()
      for p in "${pkgs[@]}"; do [[ $p == -t ]] && continue; [[ $p == pattern || $p == devel_basis ]] && pat+=("$p") || rest+=("$p"); done
      ((${#rest[@]})) && echo "  sudo zypper install ${rest[*]}"
      ((${#pat[@]})) && echo "  sudo zypper install -t pattern devel_basis"
      ;;
    *) echo "  (unknown distro) install: ${pkgs[*]}" ;;
  esac
}

status=0
if ((${#MISSING_BUILD[@]})); then
  print_cmd "Install missing BUILD dependencies:" "${MISSING_BUILD[@]}"
  status=1
fi
((${#MISSING_BUILD_OPT[@]})) && print_cmd "Optional build dependencies:" "${MISSING_BUILD_OPT[@]}"

((${#MISSING_RUNTIME[@]})) && print_cmd "Install optional RUNTIME dependencies:" "${MISSING_RUNTIME[@]}"
if [[ $DISTRO == debian ]] && ((${#MISSING_RUNTIME[@]})); then
  printf '%s  Notes: Debian/Ubuntu need "sudo dpkg --add-architecture i386" before installing :i386 packages;\n' "$C_DIM"
  printf '  ollama is installed from https://ollama.com/download on Debian/Ubuntu.%s\n' "$C_0"
fi

echo
if [[ $status == 0 ]]; then
  printf '%sBuild prerequisites satisfied.%s Next: make setup && make dev\n' "$C_OK" "$C_0"
else
  printf '%sSome build prerequisites are missing (see above).%s\n' "$C_ERR" "$C_0"
fi
exit $status
