#!/usr/bin/env python3
"""
Migrate dual logging (tracing + status_tx.send) to console_* macros.

This script:
1. Finds all status_tx.send() calls
2. Converts them to console_* macros
3. Removes preceding tracing macros if they exist
4. Adds necessary imports
"""

import re
import sys
from pathlib import Path
from typing import List, Tuple, Optional
import argparse


class LoggingMigrator:
    def __init__(self, dry_run=True, verbose=False):
        self.dry_run = dry_run
        self.verbose = verbose
        self.stats = {
            'files_processed': 0,
            'files_modified': 0,
            'replacements': 0,
            'imports_added': 0,
            'tracing_removed': 0,
        }

        # Regex patterns
        self.status_send_pattern = re.compile(
            r'let\s+_\s*=\s*(?:self\.)?status_tx\.send\('
        )

        self.tracing_macros = ['trace', 'debug', 'info', 'warn', 'error']

    def infer_log_level_from_message(self, message: str) -> str:
        """Infer log level from message content."""
        msg_upper = message.upper()
        if msg_upper.startswith('[ERROR]') or msg_upper.startswith('ERROR:') or '✗' in message:
            return 'error'
        elif msg_upper.startswith('[WARN]') or msg_upper.startswith('WARN:'):
            return 'warn'
        elif msg_upper.startswith('[DEBUG]') or msg_upper.startswith('DEBUG:'):
            return 'debug'
        elif msg_upper.startswith('[TRACE]') or msg_upper.startswith('TRACE:'):
            return 'trace'
        else:
            # Default to info for lifecycle events, arrows, etc.
            return 'info'

    def extract_status_send_message(self, line: str, lines: List[str], line_idx: int) -> Tuple[Optional[str], int]:
        """
        Extract the complete message from status_tx.send().
        Returns (message_content, end_line_idx) or (None, line_idx) if can't parse.

        Handles:
        - status_tx.send(format!("...", args))
        - status_tx.send("...".to_string())
        - status_tx.send(format!(...)) spanning multiple lines
        """
        # Try to find the complete statement
        statement = line.strip()
        current_idx = line_idx

        # Accumulate lines until we find the closing )
        paren_count = statement.count('(') - statement.count(')')
        while paren_count > 0 and current_idx + 1 < len(lines):
            current_idx += 1
            next_line = lines[current_idx].strip()
            statement += ' ' + next_line
            paren_count += next_line.count('(') - next_line.count(')')

        # Extract message
        # Pattern 1: format!("message", args)
        format_match = re.search(r'format!\s*\(\s*"([^"]*)"', statement)
        if format_match:
            msg = format_match.group(1)
            # Find the format! part
            format_start = statement.find('format!(')
            if format_start != -1:
                # Find the matching closing paren for format!()
                format_content_start = format_start + len('format!(')
                paren_count = 1
                i = format_content_start
                while i < len(statement) and paren_count > 0:
                    if statement[i] == '(':
                        paren_count += 1
                    elif statement[i] == ')':
                        paren_count -= 1
                    i += 1
                format_content = statement[format_content_start:i-1]

                # Check if there are args after the message
                # format_content is like: "message" or "message", arg1, arg2
                msg_end = format_content.find('"', 1)  # Find closing quote
                if msg_end != -1 and msg_end + 1 < len(format_content):
                    rest = format_content[msg_end+1:].strip()
                    if rest.startswith(','):
                        args = rest[1:].strip()
                        return (f'"{msg}", {args}', current_idx)

            return (f'"{msg}"', current_idx)

        # Pattern 2: "message".to_string()
        to_string_match = re.search(r'"([^"]*)"\s*\.to_string\(\)', statement)
        if to_string_match:
            msg = to_string_match.group(1)
            return (f'"{msg}"', current_idx)

        # Pattern 3: literal string
        literal_match = re.search(r'status_tx\.send\s*\(\s*"([^"]*)"\s*\)', statement)
        if literal_match:
            msg = literal_match.group(1)
            return (f'"{msg}"', current_idx)

        return (None, current_idx)

    def find_preceding_tracing_macro(self, lines: List[str], status_send_idx: int) -> Tuple[Optional[str], Optional[int], Optional[int]]:
        """
        Look backwards from status_send_idx to find a tracing macro.
        Returns (macro_name, start_line_idx, end_line_idx) or (None, None, None).
        Looks up to 5 lines back, handles multi-line macros.
        """
        for lookback in range(1, min(6, status_send_idx + 1)):
            idx = status_send_idx - lookback
            line = lines[idx].strip()

            for macro in self.tracing_macros:
                if f'{macro}!' in line:
                    # Found a tracing macro, now find where it ends (semicolon)
                    end_idx = idx
                    current_line = lines[idx]

                    # Keep looking forward until we find the semicolon
                    while end_idx < len(lines) - 1 and ';' not in current_line:
                        end_idx += 1
                        current_line = lines[end_idx]

                    return (macro, idx, end_idx)

        return (None, None, None)

    def process_file(self, filepath: Path) -> bool:
        """Process a single file. Returns True if modified."""
        self.stats['files_processed'] += 1

        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                lines = f.readlines()
        except Exception as e:
            print(f"Error reading {filepath}: {e}", file=sys.stderr)
            return False

        original_lines = lines.copy()
        modified = False
        lines_to_remove = set()  # Indices of tracing macros to remove
        replacements = []  # (line_idx, old_line, new_line, macro_level)

        # Find all status_tx.send() calls
        i = 0
        while i < len(lines):
            line = lines[i]

            if self.status_send_pattern.search(line):
                # Extract the message
                message_args, end_idx = self.extract_status_send_message(line, lines, i)

                if message_args is None:
                    if self.verbose:
                        print(f"  Skipping complex status_tx.send at {filepath}:{i+1}")
                    i += 1
                    continue

                # Check if there's a preceding tracing macro
                macro_name, macro_start_idx, macro_end_idx = self.find_preceding_tracing_macro(lines, i)

                if macro_name:
                    # Use the macro's level
                    level = macro_name
                    # Mark all lines of the tracing macro for removal
                    for idx in range(macro_start_idx, macro_end_idx + 1):
                        lines_to_remove.add(idx)
                    self.stats['tracing_removed'] += 1
                else:
                    # Infer level from message
                    # First, extract just the string part to check prefix
                    msg_match = re.search(r'"([^"]*)"', message_args)
                    if msg_match:
                        level = self.infer_log_level_from_message(msg_match.group(1))
                    else:
                        level = 'info'

                # Build the replacement line
                indent = len(line) - len(line.lstrip())
                status_tx_ref = 'self.status_tx' if 'self.status_tx' in line else 'status_tx'
                new_line = ' ' * indent + f'console_{level}!({status_tx_ref}, {message_args});\n'

                # Store replacement (handle multi-line send)
                for idx in range(i, end_idx + 1):
                    if idx not in lines_to_remove:
                        replacements.append((idx, lines[idx], new_line if idx == i else '', level))
                        if idx != i:  # Mark continuation lines for removal
                            lines_to_remove.add(idx)

                modified = True
                self.stats['replacements'] += 1

                i = end_idx + 1
            else:
                i += 1

        if not modified:
            return False

        # Apply replacements
        new_lines = []
        for i, line in enumerate(lines):
            if i in lines_to_remove:
                # Check if this is a tracing macro line we're removing
                is_tracing = any(f'{macro}!' in line for macro in self.tracing_macros)
                if is_tracing and self.verbose:
                    print(f"  Removing tracing macro at {filepath}:{i+1}")
                continue

            # Check if this line has a replacement
            replacement = next((r for r in replacements if r[0] == i), None)
            if replacement:
                _, _, new_line, level = replacement
                if new_line:  # Skip empty replacements (continuation lines)
                    new_lines.append(new_line)
            else:
                new_lines.append(line)

        # Add import if needed
        needs_import = self.add_import_if_needed(new_lines)
        if needs_import:
            self.stats['imports_added'] += 1

        # Write back if not dry-run
        if not self.dry_run:
            try:
                with open(filepath, 'w', encoding='utf-8') as f:
                    f.writelines(new_lines)
            except Exception as e:
                print(f"Error writing {filepath}: {e}", file=sys.stderr)
                return False

        self.stats['files_modified'] += 1

        if self.verbose or self.dry_run:
            print(f"✓ Modified {filepath}")
            if self.dry_run:
                # Show diff
                print(f"  Changes: {len(replacements)} replacements, {len(lines_to_remove)} lines removed")

        return True

    def add_import_if_needed(self, lines: List[str]) -> bool:
        """Add console_* macro imports if not already present. Returns True if added."""
        # Check if already imported (look for use statements only)
        for line in lines:
            stripped = line.strip()
            if stripped.startswith('use ') and ('console_trace' in line or 'console_debug' in line or 'console_info' in line):
                return False

        # Find where to insert import (after last 'use' statement)
        last_use_idx = -1
        for i, line in enumerate(lines):
            if line.strip().startswith('use ') and not line.strip().startswith('use self::'):
                last_use_idx = i

        # Build import line - use crate:: for internal imports
        import_line = 'use crate::{console_trace, console_debug, console_info, console_warn, console_error};\n'

        if last_use_idx >= 0:
            # Insert after last use statement
            lines.insert(last_use_idx + 1, import_line)
        else:
            # Find first non-comment, non-doc line
            for i, line in enumerate(lines):
                stripped = line.strip()
                if stripped and not stripped.startswith('//') and not stripped.startswith('#'):
                    lines.insert(i, import_line)
                    break

        return True

    def migrate_directory(self, directory: Path, recursive: bool = True):
        """Migrate all .rs files in a directory."""
        pattern = '**/*.rs' if recursive else '*.rs'

        rust_files = list(directory.glob(pattern))
        print(f"Found {len(rust_files)} Rust files in {directory}")

        for filepath in rust_files:
            # Skip generated files
            if 'target' in filepath.parts or 'build.rs' in filepath.name:
                continue

            self.process_file(filepath)

    def print_stats(self):
        """Print migration statistics."""
        print("\n" + "="*60)
        print("Migration Statistics")
        print("="*60)
        print(f"Files processed:     {self.stats['files_processed']}")
        print(f"Files modified:      {self.stats['files_modified']}")
        print(f"Replacements made:   {self.stats['replacements']}")
        print(f"Tracing macros removed: {self.stats['tracing_removed']}")
        print(f"Imports added:       {self.stats['imports_added']}")
        print("="*60)

        if self.dry_run:
            print("\n⚠️  DRY RUN - No files were actually modified")
            print("Run with --apply to make changes")


def main():
    parser = argparse.ArgumentParser(
        description='Migrate dual logging to console_* macros',
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog="""
Examples:
  # Dry run on entire codebase
  python scripts/migrate_dual_logging.py --all --dry-run

  # Migrate specific directory
  python scripts/migrate_dual_logging.py --dir src/server/tcp --apply

  # Migrate single file
  python scripts/migrate_dual_logging.py --file src/server/ssh/mod.rs --apply --verbose
        """
    )

    parser.add_argument('--all', action='store_true', help='Process entire src/ directory')
    parser.add_argument('--dir', type=str, help='Process specific directory')
    parser.add_argument('--file', type=str, help='Process single file')
    parser.add_argument('--apply', action='store_true', help='Apply changes (default is dry-run)')
    parser.add_argument('--verbose', '-v', action='store_true', help='Verbose output')

    args = parser.parse_args()

    # Determine dry-run mode
    dry_run = not args.apply

    migrator = LoggingMigrator(dry_run=dry_run, verbose=args.verbose)

    if args.file:
        filepath = Path(args.file)
        if not filepath.exists():
            print(f"Error: File not found: {filepath}", file=sys.stderr)
            sys.exit(1)
        migrator.process_file(filepath)
    elif args.dir:
        dirpath = Path(args.dir)
        if not dirpath.exists():
            print(f"Error: Directory not found: {dirpath}", file=sys.stderr)
            sys.exit(1)
        migrator.migrate_directory(dirpath)
    elif args.all:
        src_dir = Path('/home/user/netget/src')
        if not src_dir.exists():
            print(f"Error: Source directory not found: {src_dir}", file=sys.stderr)
            sys.exit(1)
        migrator.migrate_directory(src_dir)
    else:
        parser.print_help()
        sys.exit(1)

    migrator.print_stats()

    if dry_run:
        print("\n💡 Tip: Review the changes above, then run with --apply to make them permanent")


if __name__ == '__main__':
    main()
