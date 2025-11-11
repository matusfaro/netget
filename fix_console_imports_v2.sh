#!/bin/bash
# Script to fix console_* macro imports, handling multi-line use blocks

# Find all Rust files that use console_* macros
files=$(grep -r "console_\(trace\|debug\|info\|warn\|error\)!" src/ --include="*.rs" -l)

echo "Fixing console imports in all files..."
echo ""

import_line='use crate::{console_trace, console_debug, console_info, console_warn, console_error};'

for file in $files; do
    # Check if file has the misplaced import
    if ! grep -q "use crate::{console_trace, console_debug, console_info, console_warn, console_error}" "$file"; then
        echo "⚠ $file - no import found (might be missing)"
        continue
    fi

    # Use Python to do sophisticated parsing
    python3 <<EOF
import re

# Read the file
with open("$file", "r") as f:
    lines = f.readlines()

# Find and remove any existing console imports
new_lines = []
in_multiline_use = False
use_block_start = 0

for i, line in enumerate(lines):
    # Check if this is the console import line
    if "use crate::{console_trace, console_debug, console_info, console_warn, console_error}" in line:
        # Skip this line (we'll add it back in the right place)
        continue

    new_lines.append(line)

# Now find the right place to insert the import
# Look for the last top-level use statement before any code
insert_pos = 0
in_multiline_use = False

for i, line in enumerate(new_lines):
    stripped = line.lstrip()

    # Track multi-line use blocks
    if stripped.startswith("use ") and "{" in line and "}" not in line:
        in_multiline_use = True
    elif in_multiline_use and "}" in line:
        in_multiline_use = False
        insert_pos = i + 1  # After the closing brace
    elif stripped.startswith("use ") and not in_multiline_use:
        insert_pos = i + 1
    elif stripped.startswith(("pub ", "fn ", "impl ", "struct ", "enum ", "const ", "static ", "mod ", "trait ", "type ")):
        # Hit code, stop looking
        break

# Insert the import at the found position
if insert_pos > 0 and insert_pos < 100:  # Safety check
    new_lines.insert(insert_pos, "$import_line\n")

    # Write back
    with open("$file", "w") as f:
        f.writelines(new_lines)

    print(f"✓ $file - fixed import at line {insert_pos + 1}")
else:
    print(f"⚠ $file - could not find safe insertion point")
EOF

done

echo ""
echo "Done!"
