#!/bin/bash
# scripts/install.sh — one-line installer for the RustJVM runtime.
#
#   curl -fsSL https://rustjvm.dev/install.sh | bash

set -e

RUSTJVM_VERSION="0.1.0-alpha"

echo "Installing RustJVM runtime ${RUSTJVM_VERSION}..."

# Detect OS and architecture
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
    Linux)  PLATFORM="linux" ;;
    Darwin) PLATFORM="macos" ;;
    *)
        echo "Unsupported OS: $OS (on Windows, use scripts/install.ps1)"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64)        ARCH="amd64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *)
        echo "Unsupported architecture: $ARCH"
        exit 1
        ;;
esac

URL="https://github.com/rustjvm/rustjvm/releases/download/v${RUSTJVM_VERSION}/rustjvm-${PLATFORM}-${ARCH}"
INSTALL_DIR="${RUSTJVM_HOME:-$HOME/.rustjvm}"
mkdir -p "$INSTALL_DIR/bin"

echo "Downloading $URL ..."
curl -fsSL "$URL" -o "$INSTALL_DIR/bin/rustjvm"
chmod +x "$INSTALL_DIR/bin/rustjvm"

# Add to PATH if not already there
if [[ ":$PATH:" != *":$INSTALL_DIR/bin:"* ]]; then
    echo "export PATH=\"$INSTALL_DIR/bin:\$PATH\"" >> ~/.bashrc
    echo "export RUSTJVM_HOME=\"$INSTALL_DIR\"" >> ~/.bashrc
    echo "Added $INSTALL_DIR/bin to PATH. Restart your shell or run:"
    echo "  export PATH=\"$INSTALL_DIR/bin:\$PATH\""
fi

echo "RustJVM installed successfully!"
echo "Run 'rustjvm --version' to verify."
