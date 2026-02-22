#!/bin/bash
set -e

echo "Building OCLAWS release..."

# Get version from Cargo.toml
VERSION=$(grep '^version = ' Cargo.toml | sed 's/version = "\(.*\)"/\1/')

echo "Version: $VERSION"

# Build release
echo "Building release binary..."
cargo build --release

# Create release directory
mkdir -p release

# Copy binary
cp target/release/oclaws release/oclaws-$VERSION-linux-x64
cp target/release/oclaws release/oclaws-$VERSION-linux-arm64

# Create tarballs
cd release
tar -czvf oclaws-$VERSION-linux-x64.tar.gz oclaws-$VERSION-linux-x64
tar -czvf oclaws-$VERSION-linux-arm64.tar.gz oclaws-$VERSION-linux-arm64

echo "Release files created:"
ls -la

echo "Done!"
