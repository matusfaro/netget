#!/bin/bash

# Script to update feature gates in test mod.rs files
# Maps directory names to feature names

# Function to get feature name from directory
get_feature_name() {
    local dir="$1"
    case "$dir" in
        "tor_directory") echo "tor" ;;
        "tor_relay") echo "tor" ;;
        "tor_integration") echo "tor" ;;
        *) echo "$dir" ;;
    esac
}

# Process all test/server/*/mod.rs files
for modfile in tests/server/*/mod.rs; do
    dir=$(basename $(dirname "$modfile"))
    feature=$(get_feature_name "$dir")

    echo "Processing $modfile (feature: $feature)..."

    # Create temp file
    tmpfile=$(mktemp)

    # Process the file - all use single feature now
    sed -E "s|^#\[cfg\(test\)\]$|#[cfg(all(test, feature = \"$feature\"))]|g; \
            s|^#\[cfg\(feature = \"e2e-tests\"\)\]$|#[cfg(all(test, feature = \"$feature\"))]|g" \
        "$modfile" > "$tmpfile"

    # Replace original file
    mv "$tmpfile" "$modfile"
done

echo "Done! Updated all test mod.rs files with protocol-specific feature gates"
