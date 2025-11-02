#!/bin/bash
# Comprehensive script to replace ALL e2e-tests references with protocol-specific features

set -e

echo "=== Phase 1: Update test file cfg gates ==="
find /Users/matus/dev/netget/tests/server -name "*.rs" -type f | while IFS= read -r file; do
  if grep -q 'e2e-tests' "$file"; then
    protocol=$(echo "$file" | sed 's|.*/tests/server/\([^/]*\)/.*|\1|')
    echo "Processing: $file (protocol: $protocol)"
    
    # Replace all e2e-tests with protocol name
    sed -i '' "s|e2e-tests|$protocol|g" "$file"
    echo "  ✓ Updated"
  fi
done

echo ""
echo "=== Phase 2: Update CLAUDE.md files ==="
find /Users/matus/dev/netget/tests/server -name "CLAUDE.md" -type f | while IFS= read -r file; do
  if grep -q 'e2e-tests' "$file"; then
    protocol=$(echo "$file" | sed 's|.*/tests/server/\([^/]*\)/.*|\1|')
    echo "Processing: $file (protocol: $protocol)"
    
    # Replace --features e2e-tests,protocol with --features protocol
    sed -i '' "s|--features e2e-tests,$protocol|--features $protocol|g" "$file"
    # Replace --features e2e-tests with --features protocol
    sed -i '' "s|--features e2e-tests|--features $protocol|g" "$file"
    echo "  ✓ Updated"
  fi
done

echo ""
echo "=== Phase 3: Update README files ==="
if [ -f "/Users/matus/dev/netget/tests/README.md" ]; then
  echo "Processing: tests/README.md"
  # Keep e2e-tests in general examples, but note it's deprecated
  sed -i '' 's|--features e2e-tests|--features <protocol>|g' /Users/matus/dev/netget/tests/README.md
  echo "  ✓ Updated to use <protocol> placeholder"
fi

echo ""
echo "=== Phase 4: Update root documentation files ==="
for file in /Users/matus/dev/netget/*.md; do
  if [ -f "$file" ] && grep -q 'e2e-tests' "$file"; then
    echo "Processing: $(basename $file)"
    # Replace with generic protocol placeholder
    sed -i '' 's|--features e2e-tests|--features <protocol>|g' "$file"
    echo "  ✓ Updated to use <protocol> placeholder"
  fi
done

echo ""
echo "=== Phase 5: Update root test files ==="
# e2e_footer_test.rs - this is a UI test, not protocol-specific
if [ -f "/Users/matus/dev/netget/tests/e2e_footer_test.rs" ]; then
  echo "Skipping e2e_footer_test.rs (UI test)"
fi

# tests/server.rs - helper file
if [ -f "/Users/matus/dev/netget/tests/server.rs" ] && grep -q 'e2e-tests' "$file"; then
  echo "Processing: tests/server.rs"
  echo "  Note: This contains shared test helpers - keeping as-is"
fi

echo ""
echo "=== Phase 6: Special cases ==="
# tor_integration, tor_relay, and tor_directory - all use single "tor" feature
for file in /Users/matus/dev/netget/tests/server/tor_integration/*.rs /Users/matus/dev/netget/tests/server/tor_relay/*.rs /Users/matus/dev/netget/tests/server/tor_directory/*.rs; do
  if [ -f "$file" ] && grep -q '#\[cfg.*e2e-tests' "$file"; then
    echo "Processing: $file"
    # These use tor feature
    sed -i '' 's|feature = "e2e-tests"|feature = "tor"|g' "$file"
    echo "  ✓ Updated to use tor"
  fi
done

# Special protocol-specific cases from nested directories
for dir in /Users/matus/dev/netget/tests/server/*/; do
  protocol=$(basename "$dir")
  
  # Handle e2e test README files
  if [ -f "${dir}e2e_client_test_README.md" ]; then
    echo "Processing: ${protocol}/e2e_client_test_README.md"
    sed -i '' "s|--features e2e-tests,$protocol|--features $protocol|g" "${dir}e2e_client_test_README.md"
    sed -i '' "s|--features e2e-tests|--features $protocol|g" "${dir}e2e_client_test_README.md"
    echo "  ✓ Updated"
  fi
  
  # Handle various e2e test files
  for testfile in "${dir}"e2e*.rs; do
    if [ -f "$testfile" ] && grep -q 'e2e-tests' "$testfile"; then
      echo "Processing: ${protocol}/$(basename $testfile)"
      sed -i '' "s|e2e-tests|$protocol|g" "$testfile"
      echo "  ✓ Updated"
    fi
  done
done

echo ""
echo "=== Verification ==="
echo "Remaining e2e-tests references:"
grep -r "e2e-tests" /Users/matus/dev/netget --include="*.rs" --include="*.md" --include="*.toml" 2>/dev/null | wc -l

echo ""
echo "Done!"
