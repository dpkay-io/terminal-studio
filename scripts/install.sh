#!/usr/bin/env sh
# Installs Terminal Studio by downloading the latest release binary.
# Usage: curl -fsSL https://raw.githubusercontent.com/dpkay-io/terminal-studio/master/scripts/install.sh | sh
set -eu

REPO="dpkay-io/terminal-studio"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

os=$(uname -s | tr '[:upper:]' '[:lower:]')
arch=$(uname -m)

case "$os" in
  linux)
    asset="terminal-studio-linux"
    ;;
  darwin)
    asset="terminal-studio-macos-arm"
    ;;
  *)
    echo "Unsupported OS: $os. Build from source: https://github.com/$REPO" >&2
    exit 1
    ;;
esac

url="https://github.com/${REPO}/releases/latest/download/${asset}"

printf 'Downloading Terminal Studio (%s)...\n' "$asset"
mkdir -p "$INSTALL_DIR"
curl -fsSL "$url" -o "$INSTALL_DIR/terminal-studio"
chmod +x "$INSTALL_DIR/terminal-studio"

printf 'Installed to %s/terminal-studio\n' "$INSTALL_DIR"

case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *)
    printf '\nNote: %s is not in your PATH. Add it with:\n' "$INSTALL_DIR"
    printf '  export PATH="$PATH:%s"\n' "$INSTALL_DIR"
    ;;
esac
