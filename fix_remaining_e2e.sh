#!/bin/bash
set -e

echo "Fixing remaining e2e-tests references..."

# 1. tests/e2e_footer_test.rs - UI test, remove feature gate entirely (it's not protocol-specific)
echo "Removing feature gate from e2e_footer_test.rs (UI test)"
sed -i '' '/#!\[cfg(feature = "e2e-tests")\]/d' /Users/matus/dev/netget/tests/e2e_footer_test.rs

# 2. tests/server.rs - Helper module, remove feature gates (always compile)
echo "Removing feature gates from tests/server.rs (helper module)"
sed -i '' '/#\[cfg(feature = "e2e-tests")\]/d' /Users/matus/dev/netget/tests/server.rs

# 3. datalink/CLAUDE.md - Update documentation example
echo "Updating datalink/CLAUDE.md"
sed -i '' 's|e2e-tests-privileged = \["e2e-tests"\]|e2e-tests-privileged = \["datalink"\]|g' /Users/matus/dev/netget/tests/server/datalink/CLAUDE.md

# 4. wireguard/CLAUDE.md - Update code example
echo "Updating wireguard/CLAUDE.md"
sed -i '' 's|feature = "e2e-tests"|feature = "wireguard"|g' /Users/matus/dev/netget/tests/server/wireguard/CLAUDE.md

echo "Done!"
echo ""
echo "Verification:"
grep -r "e2e-tests" /Users/matus/dev/netget --include="*.rs" --include="*.md" 2>/dev/null | grep -v ".git" | wc -l
