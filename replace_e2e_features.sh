#!/bin/bash
# Script to replace #![cfg(feature = "e2e-tests")] with protocol-specific features
# in all test files

set -e

# Find all .rs files in tests/server that contain the e2e-tests feature
find /Users/matus/dev/netget/tests/server -name "*.rs" -type f | while IFS= read -r file; do
  if grep -q '#!\[cfg(feature = "e2e-tests")\]' "$file"; then
    # Extract protocol name from path: tests/server/PROTOCOL/...
    protocol=$(echo "$file" | sed 's|.*/tests/server/\([^/]*\)/.*|\1|')

    echo "Processing: $file (protocol: $protocol)"

    # Replace the feature gate
    sed -i '' 's/#!\[cfg(feature = "e2e-tests")\]/#![cfg(feature = "'"$protocol"'")]/' "$file"

    echo "  ✓ Updated to use feature '$protocol'"
  fi
done

echo ""
echo "Done! All test files updated."
