#!/bin/sh
set -eu

REPO="KikotVit/pact-lang"
INSTALL_DIR="${PACT_INSTALL_DIR:-/usr/local/bin}"

OS=$(uname -s)
ARCH=$(uname -m)

case "${OS}" in
  Darwin)
    case "${ARCH}" in
      arm64) TARGET="aarch64-apple-darwin" ;;
      x86_64) TARGET="x86_64-apple-darwin" ;;
      *) echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  Linux)
    case "${ARCH}" in
      x86_64) TARGET="x86_64-unknown-linux-gnu" ;;
      *) echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;;
    esac
    ;;
  *) echo "Unsupported OS: ${OS}" >&2; exit 1 ;;
esac

VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')

if [ -z "${VERSION}" ]; then
  echo "Error: could not determine latest release." >&2
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/pact-${TARGET}.tar.gz"

echo "Downloading pact ${VERSION} for ${TARGET}..."

TMPDIR=$(mktemp -d)
trap 'rm -rf "${TMPDIR}"' EXIT

curl -fsSL "${URL}" -o "${TMPDIR}/pact.tar.gz"
tar xzf "${TMPDIR}/pact.tar.gz" -C "${TMPDIR}"

install -m 755 "${TMPDIR}/pact" "${INSTALL_DIR}/pact"

echo "pact ${VERSION} installed to ${INSTALL_DIR}/pact"
pact --version 2>/dev/null || true
