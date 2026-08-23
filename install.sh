#!/usr/bin/env sh
set -eu

REPO="francis-du/wcode"
INSTALL_DIR="${WCODE_INSTALL_DIR:-$HOME/.local/bin}"
TMP_DIR="${TMPDIR:-/tmp}/wcode-install-$$"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT INT TERM

command_exists() {
  command -v "$1" >/dev/null 2>&1
}

fail() {
  printf 'wcode install: %s\n' "$1" >&2
  exit 1
}

version="${WCODE_VERSION:-latest}"
case "$version" in
  latest)
    BASE_URL="https://github.com/${REPO}/releases/latest/download"
    ;;
  *[!A-Za-z0-9._-]*|'')
    fail "invalid WCODE_VERSION: $version"
    ;;
  *)
    BASE_URL="https://github.com/${REPO}/releases/download/${version}"
    ;;
esac

if ! command_exists curl; then
  fail "curl is required"
fi

os=$(uname -s)
arch=$(uname -m)

case "$os" in
  Linux)
    case "$arch" in
      x86_64|amd64)
        archive="wcode-linux-x86_64.tar.gz"
        ;;
      *)
        fail "unsupported Linux architecture: $arch"
        ;;
    esac
    ;;
  Darwin)
    case "$arch" in
      arm64|aarch64)
        archive="wcode-macos-aarch64.tar.gz"
        ;;
      x86_64|amd64)
        archive="wcode-macos-x86_64.tar.gz"
        ;;
      *)
        fail "unsupported macOS architecture: $arch"
        ;;
    esac
    ;;
  *)
    fail "unsupported operating system: $os (use install.ps1 on Windows)"
    ;;
esac

mkdir -p "$TMP_DIR" "$INSTALL_DIR"

printf 'Downloading %s...\n' "$archive"
curl -fL --retry 3 --connect-timeout 10 \
  "$BASE_URL/$archive" \
  -o "$TMP_DIR/$archive"

printf 'Downloading checksums...\n'
curl -fL --retry 3 --connect-timeout 10 \
  "$BASE_URL/SHA256SUMS" \
  -o "$TMP_DIR/SHA256SUMS"

expected=$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1; exit }' "$TMP_DIR/SHA256SUMS")
[ -n "$expected" ] || fail "checksum for $archive was not found"

if command_exists sha256sum; then
  actual=$(sha256sum "$TMP_DIR/$archive" | awk '{print $1}')
elif command_exists shasum; then
  actual=$(shasum -a 256 "$TMP_DIR/$archive" | awk '{print $1}')
else
  fail "sha256sum or shasum is required to verify the download"
fi

[ "$actual" = "$expected" ] || fail "SHA-256 checksum mismatch"

mkdir -p "$TMP_DIR/package"
tar -xzf "$TMP_DIR/$archive" -C "$TMP_DIR/package"

binary=$(find "$TMP_DIR/package" -type f -name wcode -print | head -n 1)
[ -n "$binary" ] || fail "wcode binary was not found in the release archive"

install_path="$INSTALL_DIR/wcode"
cp "$binary" "$install_path"
chmod 755 "$install_path"

printf '\nInstalled wcode to %s\n' "$install_path"

case ":${PATH:-}:" in
  *":$INSTALL_DIR:"*)
    ;;
  *)
    printf '\nAdd this directory to PATH if needed:\n  %s\n' "$INSTALL_DIR"
    ;;
esac

"$install_path" --version
