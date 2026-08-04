#!/usr/bin/env bash
# Install jmux into a prefix using the layout the app expects:
#
#   $PREFIX/bin/jmux-app                      GUI binary
#   $PREFIX/bin/jmux                          CLI (bash wrapper)
#   $PREFIX/bin/jmux-cli                      typed CLI client (Rust)
#   $PREFIX/share/jmux/shell-integration/     zsh/bash/fish integration (+ .zshenv)
#   $PREFIX/share/jmux/bin/                    jmux/claude/xdg-open (PATH-injected in jmux terminals)
#   $PREFIX/share/applications/…desktop       launcher entry
#   $PREFIX/share/metainfo/…metainfo.xml      AppStream metadata
#   $PREFIX/share/icons/hicolor/scalable/…    bundled symbolic icons
#
# The GUI resolves shell-integration via <exe_dir>/../share/jmux/shell-integration,
# so this layout works without any extra configuration.
#
# Usage:
#   bash scripts/install.sh [PREFIX]      # default PREFIX=/usr/local (needs root)
#   bash scripts/install.sh "$HOME/.local"  # per-user, no root required
set -euo pipefail

PREFIX="${1:-/usr/local}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REL="$REPO_ROOT/target/release"

if [[ ! -x "$REL/jmux-app" ]]; then
  echo "error: $REL/jmux-app not found — build first with:" >&2
  echo "  cargo build --release --features jmux/link-ghostty" >&2
  exit 1
fi

echo "Installing jmux to $PREFIX"

# Surface which build is being installed. The quick-terminal (quake) drop-down
# is an opt-in cargo feature, and install.sh copies whatever was last built — so
# a stray default build silently ships without the global hotkey/portal. Make
# that state visible instead of mysterious.
if grep -aqF 'org.freedesktop.portal.GlobalShortcuts' "$REL/jmux-app" 2>/dev/null; then
  echo "  quick-terminal: ENABLED in this build"
else
  echo "  quick-terminal: NOT in this build — rebuild with"
  echo "    cargo build --release --features jmux/link-ghostty,jmux/quick-terminal"
fi

install -Dm755 "$REL/jmux-app" "$PREFIX/bin/jmux-app"

# Bundled runtime libraries (vendored ghostty + friends). The binary's rpath
# prefers $ORIGIN/../lib/jmux, so these — not any system copy from an old
# package — are what the loader resolves. Skipping this step would silently
# fall back to /usr/lib versions with long-fixed bugs.
dest_lib="$PREFIX/lib/jmux"
mkdir -p "$dest_lib"
for so in "$REL"/libghostty.so "$REL"/libghostty-vt.so* "$REL"/libglad.so; do
  [[ -e "$so" ]] && install -Dm755 "$so" "$dest_lib/$(basename "$so")"
done
# Bash CLI wrapper is the canonical `jmux` (matches the in-terminal PATH prepend).
install -Dm755 "$REPO_ROOT/jmux/bin/jmux" "$PREFIX/bin/jmux"
# Typed Rust CLI client (optional companion).
[[ -x "$REL/jmux-cli" ]] && install -Dm755 "$REL/jmux-cli" "$PREFIX/bin/jmux-cli"

# Shell integration (preserve dotfiles like .zshenv and the fish/ subtree).
dest_si="$PREFIX/share/jmux/shell-integration"
rm -rf "$dest_si"
mkdir -p "$dest_si"
cp -a "$REPO_ROOT/jmux/shell-integration/." "$dest_si/"

# Bundled bin scripts (jmux/claude/xdg-open) for the in-terminal PATH prepend.
dest_bin="$PREFIX/share/jmux/bin"
mkdir -p "$dest_bin"
cp -a "$REPO_ROOT/jmux/bin/." "$dest_bin/"
chmod 755 "$dest_bin"/*

# Desktop entry + AppStream metadata.
install -Dm644 "$REPO_ROOT/data/com.jacobbriggs.jmux.desktop" \
  "$PREFIX/share/applications/com.jacobbriggs.jmux.desktop"
install -Dm644 "$REPO_ROOT/data/com.jacobbriggs.jmux.metainfo.xml" \
  "$PREFIX/share/metainfo/com.jacobbriggs.jmux.metainfo.xml"

# Bundled symbolic icons into the hicolor theme so GTK resolves them.
if [[ -d "$REPO_ROOT/jmux/icons/scalable" ]]; then
  while IFS= read -r -d '' svg; do
    rel="${svg#"$REPO_ROOT"/jmux/icons/scalable/}"
    install -Dm644 "$svg" "$PREFIX/share/icons/hicolor/scalable/$rel"
  done < <(find "$REPO_ROOT/jmux/icons/scalable" -name '*.svg' -print0)
fi

# Application icon(s) into the hicolor theme (matches the desktop Icon=).
# Install both the scalable SVG and the sized PNG renditions — some docks /
# taskbars only resolve a pinned (not-running) launcher's icon via the PNG
# sizes in the icon cache, showing a generic icon otherwise.
if [[ -d "$REPO_ROOT/data/icons/hicolor" ]]; then
  while IFS= read -r -d '' icon; do
    rel="${icon#"$REPO_ROOT"/data/icons/hicolor/}"
    install -Dm644 "$icon" "$PREFIX/share/icons/hicolor/$rel"
  done < <(find "$REPO_ROOT/data/icons/hicolor" \
    \( -name '*.svg' -o -name '*.png' -o -name 'index.theme' \) -print0)
  # Pixmaps fallback — some launchers look here directly by Icon= name.
  install -Dm644 "$REPO_ROOT/data/icons/hicolor/256x256/apps/com.jacobbriggs.jmux.png" \
    "$PREFIX/share/pixmaps/com.jacobbriggs.jmux.png" 2>/dev/null || true
fi

# Refresh caches (best-effort).
command -v update-desktop-database >/dev/null 2>&1 && \
  update-desktop-database "$PREFIX/share/applications" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && \
  gtk-update-icon-cache -qtf "$PREFIX/share/icons/hicolor" 2>/dev/null || true

# NOTE: do NOT rebuild the user's KDE sycoca/icon cache from this (root)
# installer — running kbuildsycoca without the user's session environment can
# corrupt their caches. KDE/Plasma will pick up the new icon on the next login,
# or the user can run `kbuildsycoca6` themselves from within their session.

echo "Done."
echo "  GUI : $PREFIX/bin/jmux-app"
echo "  CLI : $PREFIX/bin/jmux"
case ":$PATH:" in
  *":$PREFIX/bin:"*) ;;
  *) echo "  note: $PREFIX/bin is not on your PATH — add it to run 'jmux-app' by name." ;;
esac
