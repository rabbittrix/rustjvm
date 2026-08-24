#!/bin/bash
# scripts/install.sh — installer for the RustJVM runtime (Linux/macOS).
#
#   scripts/install.sh --from-source                 # from a repo checkout (works today)
#   curl -fsSL https://rustjvm.dev/install.sh | bash # once the first release is tagged

set -e

RUSTJVM_VERSION="0.1.0-alpha"
FROM_SOURCE=0
for arg in "$@"; do
    case "$arg" in
        --from-source) FROM_SOURCE=1 ;;
        *) echo "Unknown option: $arg"; exit 1 ;;
    esac
done

INSTALL_DIR="${RUSTJVM_HOME:-$HOME/.rustjvm}"
mkdir -p "$INSTALL_DIR/bin"

if [ "$FROM_SOURCE" = "1" ]; then
    REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
    echo "Building RustJVM runtime from $REPO_ROOT (release mode)..."
    cargo build --release -p rustjvm-cli --manifest-path "$REPO_ROOT/Cargo.toml"
    cp "$REPO_ROOT/target/release/rustjvm" "$INSTALL_DIR/bin/rustjvm"
else
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

    echo "Installing RustJVM runtime ${RUSTJVM_VERSION}..."
    echo "Downloading $URL ..."
    curl -fsSL "$URL" -o "$INSTALL_DIR/bin/rustjvm"
    chmod +x "$INSTALL_DIR/bin/rustjvm"
fi

# Add to PATH if not already there
if [[ ":$PATH:" != *":$INSTALL_DIR/bin:"* ]]; then
    echo "export PATH=\"$INSTALL_DIR/bin:\$PATH\"" >> ~/.bashrc
    echo "export RUSTJVM_HOME=\"$INSTALL_DIR\"" >> ~/.bashrc
    echo "Added $INSTALL_DIR/bin to PATH. Restart your shell or run:"
    echo "  export PATH=\"$INSTALL_DIR/bin:\$PATH\""
fi

echo "RustJVM installed: $INSTALL_DIR/bin/rustjvm"
"$INSTALL_DIR/bin/rustjvm" --version
