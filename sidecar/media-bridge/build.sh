#!/bin/sh
# Build the macOS media sidecar. Output binary must sit next to this
# script (the Rust adapter looks for ../sidecar/media-bridge/media-bridge).
cd "$(dirname "$0")"
swiftc -O main.swift -o media-bridge
echo "built: $(pwd)/media-bridge"
