#!/bin/bash
# Script to update cargo commands to use cargo-isolated.sh wrapper in markdown files
# This ensures all Claude documentation uses isolated build directories

set -e

echo "Updating cargo commands in markdown files..."

# Find all markdown files (excluding target directories)
find . -name "*.md" -not -path "./target/*" -not -path "./target-claude/*" -type f | while read -r file; do
    # Skip if file doesn't contain cargo commands
    if ! grep -q "cargo \(build\|test\|check\|run\|clean\)" "$file"; then
        continue
    fi

    echo "Processing: $file"

    # Create backup
    cp "$file" "$file.bak"

    # Replace cargo commands with wrapper (using Perl for better regex support on macOS)
    # Pattern 1: At start of line
    perl -i -pe 's/^cargo (build|test|check|run|clean)/.\/cargo-isolated.sh $1/g' "$file"

    # Pattern 2: After space (but not ./cargo)
    perl -i -pe 's/(\s)cargo (build|test|check|run|clean)/$1.\/cargo-isolated.sh $2/g' "$file"

    # Pattern 3: After ` (backtick)
    perl -i -pe 's/`cargo (build|test|check|run|clean)/`.\/cargo-isolated.sh $1/g' "$file"

    # Pattern 4: After $ (command prompt)
    perl -i -pe 's/\$cargo (build|test|check|run|clean)/\$.\/cargo-isolated.sh $1/g' "$file"

    # Pattern 5: In markdown links/references [text](cargo ...)
    perl -i -pe 's/\(cargo (build|test|check|run|clean)/(.\/cargo-isolated.sh $1/g' "$file"

    # Check if file changed
    if ! diff -q "$file" "$file.bak" > /dev/null 2>&1; then
        echo "  ✓ Updated"
    else
        echo "  - No changes needed"
    fi

    # Remove backup
    rm "$file.bak"
done

echo ""
echo "Done! Updated cargo commands to use ./cargo-isolated.sh wrapper."
