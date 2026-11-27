#!/bin/sh
# Sets up Castable on macOS or Linux, then opens the setup wizard.
#
#   ./scripts/setup.sh
#
# Checks for Rust, Node 22+ and pnpm (offering to install what's missing),
# builds the receiver, and runs `castable setup`. Safe to run again.
set -eu
cd "$(dirname "$0")/.."

say() { printf '\n\033[1m%s\033[0m\n' "$1"; }
have() { command -v "$1" >/dev/null 2>&1; }
ask() { printf '%s [Y/n] ' "$1"; read -r reply </dev/tty || reply=n; [ -z "$reply" ] || [ "$reply" = y ] || [ "$reply" = Y ]; }

say "1/4 Rust"
if ! have cargo && [ -x "$HOME/.cargo/bin/cargo" ]; then PATH="$HOME/.cargo/bin:$PATH"; fi
if have cargo; then
  cargo --version
elif ask "Rust isn't installed. Install it now with rustup?"; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
  PATH="$HOME/.cargo/bin:$PATH"
else
  echo "Install Rust from https://rustup.rs, then run this again."; exit 1
fi

say "2/4 Node and pnpm"
if ! have node || [ "$(node -p 'process.versions.node.split(".")[0]')" -lt 22 ]; then
  case "$(uname)" in
    Darwin) echo "Node 22 or newer is needed: brew install node@22 (or use nvm)" ;;
    *) echo "Node 22 or newer is needed: https://nodejs.org, or your package manager / nvm" ;;
  esac
  exit 1
fi
node --version
have pnpm || corepack enable || { echo "Run: npm install -g pnpm"; exit 1; }
pnpm install --silent

say "3/4 A player (optional)"
if have mpv || have vlc || [ -d /Applications/IINA.app ] || [ -d /Applications/VLC.app ]; then
  echo "Found a player."
else
  case "$(uname)" in
    Darwin) echo "For full control, install one: brew install mpv   (or IINA / VLC from their websites)" ;;
    *) echo "For full control, install one: sudo apt install mpv   (or your package manager)" ;;
  esac
  echo "You can also pick QuickTime, ffplay or your browser in the wizard."
fi

say "4/4 The receiver"
cargo build --release -p castable-players --bin castable-receiver

say "Opening the setup wizard"
exec node packages/cli/bin/castable.mjs setup
