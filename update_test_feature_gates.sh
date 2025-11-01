#!/bin/bash

# Script to update feature gates in test mod.rs files
# Maps directory names to feature names

# Function to get feature name from directory
get_feature_name() {
    local dir="$1"
    case "$dir" in
        "tor_directory") echo "tor-directory" ;;
        "tor_relay") echo "tor-relay" ;;
        "tor_integration") echo "tor-directory,feature=\"tor-relay" ;;  # Special case: needs both
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

    # Process the file
    if [ "$dir" = "tor_integration" ]; then
        # Special case for tor_integration - needs both features
        sed -E "s|^#\[cfg\(test\)\]$|#[cfg(all(test, feature = \"tor-directory\", feature = \"tor-relay\"))]|g; \
                s|^#\[cfg\(feature = \"e2e-tests\"\)\]$|#[cfg(all(test, feature = \"tor-directory\", feature = \"tor-relay\"))]|g" \
            "$modfile" > "$tmpfile"
    else
        # Normal case - single feature
        sed -E "s|^#\[cfg\(test\)\]$|#[cfg(all(test, feature = \"$feature\"))]|g; \
                s|^#\[cfg\(feature = \"e2e-tests\"\)\]$|#[cfg(all(test, feature = \"$feature\"))]|g" \
            "$modfile" > "$tmpfile"
    fi

    # Replace original file
    mv "$tmpfile" "$modfile"
done

echo "Done! Updated all test mod.rs files with protocol-specific feature gates"
