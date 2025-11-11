#!/usr/bin/env python3
"""Add console_* macro imports to files that use them but don't have the import."""

import re
import sys
from pathlib import Path

def add_import_to_file(filepath):
    """Add console import if file uses console_* macros but doesn't have the import."""
    with open(filepath, 'r', encoding='utf-8') as f:
        lines = f.readlines()

    # Check if file uses console_* macros
    uses_console = any(re.search(r'console_(trace|debug|info|warn|error)!', line) for line in lines)
    if not uses_console:
        return False

    # Check if import already exists
    has_import = any(line.strip().startswith('use ') and 'console_' in line for line in lines)
    if has_import:
        return False

    # Find last use statement
    last_use_idx = -1
    for i, line in enumerate(lines):
        if line.strip().startswith('use ') and not line.strip().startswith('use self::'):
            last_use_idx = i

    if last_use_idx == -1:
        print(f"Warning: No use statements found in {filepath}", file=sys.stderr)
        return False

    # Insert import
    import_line = 'use crate::{console_trace, console_debug, console_info, console_warn, console_error};\n'
    lines.insert(last_use_idx + 1, import_line)

    # Write back
    with open(filepath, 'w', encoding='utf-8') as f:
        f.writelines(lines)

    return True

def main():
    src_dir = Path('/home/user/netget/src')
    count = 0

    for filepath in src_dir.glob('**/*.rs'):
        if filepath.name == 'build.rs':
            continue
        if add_import_to_file(filepath):
            print(f"Added import to {filepath}")
            count += 1

    print(f"\nAdded imports to {count} files")

if __name__ == '__main__':
    main()
