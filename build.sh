#!/bin/bash

echo "🔨 Building Zed Ink extension..."
cd /Users/phenry/code/zed-ink

# Build the extension
cargo build --release

if [ $? -eq 0 ]; then
    echo "✅ Build successful!"
    echo ""
    echo "📦 Extension files:"
    echo "   - Extension binary: target/release/libzed_ink.dylib"
    echo "   - Extension config: extension.toml"
    echo "   - Language config: languages/ink/config.toml"
    echo ""
    echo "🚀 Ready to install in Zed!"
else
    echo "❌ Build failed!"
    exit 1
fi