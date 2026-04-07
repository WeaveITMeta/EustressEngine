#!/bin/bash
# Generate multi-size icons from the source icon.png
# Requires: ImageMagick (convert command)
#
# Run from the repo root:
#   bash eustress/crates/engine/assets/linux/generate-icons.sh

set -e

SOURCE="eustress/crates/engine/assets/icon.png"
OUTPUT_DIR="eustress/crates/engine/assets/linux/icons"

if [ ! -f "$SOURCE" ]; then
    echo "ERROR: Source icon not found at $SOURCE"
    echo "Run this script from the repository root."
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

for size in 16 32 48 128 256 512; do
    convert "$SOURCE" -resize "${size}x${size}" "$OUTPUT_DIR/eustress-engine-${size}.png"
    echo "Generated ${size}x${size} → $OUTPUT_DIR/eustress-engine-${size}.png"
done

echo "Done. Icons ready for packaging."
