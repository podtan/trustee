#!/bin/bash
# Cross-compile trustee for x86_64-unknown-linux-gnu
# This builds the binary locally without deployment

set -e

echo "🔨 Cross-compiling trustee for x86_64-unknown-linux-gnu..."

# Install cargo-zigbuild if not already installed
if ! command -v cargo-zigbuild &> /dev/null; then
    echo "📦 Installing cargo-zigbuild..."
    cargo install cargo-zigbuild
fi

# Add x86_64 target if not already added
rustup target add x86_64-unknown-linux-gnu 2>/dev/null || true

# Cross-compile for x86_64
echo "⚙️  Building for x86_64-unknown-linux-gnu..."
cargo zigbuild --release --target x86_64-unknown-linux-gnu

# Create VERSION file
echo "📝 Creating version file..."
TRUSTEE_VERSION=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[] | select(.name=="trustee") | .version')
COMMIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

mkdir -p target/x86_64-unknown-linux-gnu/release
echo "${TRUSTEE_VERSION}-${COMMIT_SHA}" > target/x86_64-unknown-linux-gnu/release/trustee.VERSION

echo "✅ Build completed successfully!"
echo ""
echo "📦 Binary location:"
echo "   target/x86_64-unknown-linux-gnu/release/trustee"
echo ""
echo "📄 Version:"
cat target/x86_64-unknown-linux-gnu/release/trustee.VERSION
