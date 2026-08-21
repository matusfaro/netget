#!/usr/bin/env python3
"""
Fix all protocol implementations by splitting Protocol and Client/Server trait methods.

This script properly parses Rust impl blocks and separates Protocol trait methods
into their own impl block.
"""

import re
import sys
from pathlib import Path
from typing import List, Tuple, Optional

# Protocol trait methods (from protocol_trait.rs)
PROTOCOL_METHODS = {
    'get_startup_parameters',
    'get_async_actions',
    'get_sync_actions',
    'protocol_name',
    'get_event_types',
    'stack_name',
    'keywords',
    'metadata',
    'description',
    'example_prompt',
    'group_name',
    'get_dependencies',
}

# Client/Server specific methods
CLIENT_METHODS = {'connect', 'execute_action'}
SERVER_METHODS = {'spawn', 'execute_action'}


def find_matching_brace(lines: List[str], start_line: int, start_col: int) -> Tuple[int, int]:
    """Find the matching closing brace for an opening brace."""
    count = 1
    line_idx = start_line
    col = start_col + 1

    while line_idx < len(lines) and count > 0:
        line = lines[line_idx]
        while col < len(line):
            if line[col] == '{':
                count += 1
            elif line[col] == '}':
                count -= 1
                if count == 0:
                    return line_idx, col
            col += 1
        line_idx += 1
        col = 0

    raise ValueError("No matching closing brace found")


def extract_method(lines: List[str], start_idx: int) -> Tuple[str, int]:
    """Extract a complete method starting at start_idx."""
    # Find the opening brace of the method
    line = lines[start_idx]
    if '{' not in line:
        # Multi-line function signature
        idx = start_idx
        while idx < len(lines) and '{' not in lines[idx]:
            idx += 1
        if idx >= len(lines):
            raise ValueError(f"No opening brace found for method at line {start_idx}")
    else:
        idx = start_idx

    # Find position of first {
    brace_col = lines[idx].index('{')
    end_line, end_col = find_matching_brace(lines, idx, brace_col)

    # Extract all lines from start to end
    if start_idx == end_line:
        return lines[start_idx][:(end_col+1)], end_line + 1
    else:
        method_lines = [lines[start_idx]]
        for i in range(start_idx + 1, end_line):
            method_lines.append(lines[i])
        method_lines.append(lines[end_line][:end_col+1])
        return '\n'.join(method_lines), end_line + 1


def fix_protocol_file(filepath: Path) -> bool:
    """Fix a single protocol actions.rs file."""
    print(f"Processing {filepath.relative_to(Path.cwd())}...")

    with open(filepath, 'r') as f:
        content = f.read()

    # Skip if already fixed
    if 'impl Protocol for' in content:
        print(f"  ✓ Already has 'impl Protocol for', skipping")
        return False

    lines = content.split('\n')

    # Detect Client or Server
    is_client = '/client/' in str(filepath)
    trait_type = 'Client' if is_client else 'Server'

    # Find impl Client/Server block
    impl_pattern = rf'impl {trait_type} for (\w+)'
    impl_start = None
    struct_name = None

    for idx, line in enumerate(lines):
        match = re.search(impl_pattern, line)
        if match:
            impl_start = idx
            struct_name = match.group(1)
            break

    if impl_start is None:
        print(f"  ⚠️  No 'impl {trait_type} for' found")
        return False

    # Find the opening brace (might be on next line)
    brace_line = impl_start
    while brace_line < len(lines) and '{' not in lines[brace_line]:
        brace_line += 1

    if brace_line >= len(lines):
        print(f"  ⚠️  No opening brace found for impl block")
        return False

    brace_col = lines[brace_line].index('{')
    impl_end_line, impl_end_col = find_matching_brace(lines, brace_line, brace_col)

    # Parse methods within the impl block
    protocol_methods = []
    trait_methods = []

    idx = brace_line + 1
    while idx < impl_end_line:
        line = lines[idx].strip()

        # Skip empty lines and comments
        if not line or line.startswith('//') or line.startswith('/*'):
            idx += 1
            continue

        # Check for method definition
        method_match = re.match(r'fn\s+(\w+)', line)
        if method_match:
            method_name = method_match.group(1)
            try:
                method_text, next_idx = extract_method(lines, idx)

                # Categorize method
                if method_name in PROTOCOL_METHODS:
                    protocol_methods.append(method_text)
                else:
                    trait_methods.append(method_text)

                idx = next_idx
            except Exception as e:
                print(f"  ⚠️  Error extracting method {method_name}: {e}")
                idx += 1
        else:
            idx += 1

    if not protocol_methods:
        print(f"  ⚠️  No Protocol methods found to split")
        return False

    # Build new impl blocks
    indent = '    '
    protocol_impl = f"// Implement Protocol trait (common functionality)\nimpl Protocol for {struct_name} {{\n"
    for method in protocol_methods:
        # Add proper indentation to each line
        for line in method.split('\n'):
            protocol_impl += f"{indent}{line}\n"
    protocol_impl += "}\n"

    trait_impl = f"// Implement {trait_type} trait ({trait_type.lower()}-specific functionality)\nimpl {trait_type} for {struct_name} {{\n"
    for method in trait_methods:
        for line in method.split('\n'):
            trait_impl += f"{indent}{line}\n"
    trait_impl += "}\n"

    # Replace the original impl block
    before = '\n'.join(lines[:impl_start])
    after = '\n'.join(lines[impl_end_line+1:])

    new_content = before + '\n' + protocol_impl + '\n' + trait_impl + '\n' + after

    # Add Protocol import if missing
    if 'protocol_trait::Protocol' not in new_content:
        if is_client:
            new_content = new_content.replace(
                'use crate::llm::actions::{\n    client_trait::{Client, ClientActionResult},',
                'use crate::llm::actions::{\n    client_trait::{Client, ClientActionResult},\n    protocol_trait::Protocol,'
            )
        else:
            new_content = new_content.replace(
                'protocol_trait::{ActionResult, Server}',
                'protocol_trait::{ActionResult, Protocol, Server}'
            )

    # Write back
    with open(filepath, 'w') as f:
        f.write(new_content)

    print(f"  ✓ Fixed (found {len(protocol_methods)} Protocol methods, {len(trait_methods)} {trait_type} methods)")
    return True


def main():
    repo_root = Path.cwd()

    # Find all action files
    client_files = list(repo_root.glob('src/client/*/actions.rs'))
    server_files = list(repo_root.glob('src/server/*/actions.rs'))

    all_files = sorted(client_files + server_files)

    print(f"Found {len(all_files)} protocol action files\n")

    fixed_count = 0
    for filepath in all_files:
        try:
            if fix_protocol_file(filepath):
                fixed_count += 1
        except Exception as e:
            print(f"  ❌ Error processing {filepath}: {e}")
            import traceback
            traceback.print_exc()

    print(f"\n✅ Fixed {fixed_count}/{len(all_files)} files")
    return 0 if fixed_count > 0 else 1


if __name__ == '__main__':
    sys.exit(main())
