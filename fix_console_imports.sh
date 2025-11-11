#!/bin/bash
# Script to properly add console_* macro imports to all files that use them

# Find all Rust files that use console_* macros
files=$(grep -r "console_\(trace\|debug\|info\|warn\|error\)!" src/ --include="*.rs" -l)

echo "Processing files with console_* macros..."
echo ""

import_line='use crate::{console_trace, console_debug, console_info, console_warn, console_error};'

for file in $files; do
    # First, remove any existing console imports (they might be in wrong place)
    if grep -q "use crate::{console_trace, console_debug, console_info, console_warn, console_error}" "$file"; then
        # Remove the line
        sed -i.bak '/use crate::{console_trace, console_debug, console_info, console_warn, console_error};/d' "$file"
        rm "${file}.bak"
    fi

    # Find the last top-level use statement (before any fn, impl, struct, etc.)
    # We look for use statements that are NOT indented (start of line)
    last_use_line=$(awk '/^use / {line=NR} /^(pub )?(fn|impl|struct|enum|const|static|mod|trait|type) / {if (line) {print line; exit}}' "$file")

    # If we didn't find a good spot, try to find any use statement in first 50 lines
    if [ -z "$last_use_line" ]; then
        last_use_line=$(head -50 "$file" | grep -n "^use " | tail -1 | cut -d: -f1)
    fi

    # If still no use statement found, skip
    if [ -z "$last_use_line" ]; then
        echo "⚠ $file - could not find good location for import, skipping"
        continue
    fi

    # Make sure we're not past line 100 (probably inside a function)
    if [ "$last_use_line" -gt 100 ]; then
        echo "⚠ $file - last use statement too far down (line $last_use_line), skipping"
        continue
    fi

    # Insert the import line after the last use statement
    sed -i.bak "${last_use_line}a\\
${import_line}
" "$file"

    # Remove backup file
    rm "${file}.bak"

    echo "✓ $file - added import at line $((last_use_line + 1))"
done

echo ""
echo "Done! All files processed."
