#!/usr/bin/env python3
"""
Migration script to update EventType::new() calls to the new signature.

Old: EventType::new("id", "description")
New: EventType::new("id", "description", json!({"type": "placeholder"}))

This adds a placeholder that tests will detect, guiding migration.
"""

import os
import re
import sys

def process_file(filepath):
    """Process a single Rust file."""
    with open(filepath, 'r') as f:
        content = f.read()

    original = content

    # Pattern to match EventType::new("id", "description") - two string arguments
    # We need to be careful not to match EventType::new that already has 3 args

    # Match the pattern: EventType::new("...", "...")
    # followed by either ) or .with_xxx or whitespace then .
    pattern = r'EventType::new\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*\)'

    def replacement(match):
        event_id = match.group(1)
        description = match.group(2)
        # Create a placeholder example - use send_data as generic placeholder
        placeholder = f'json!({{"type": "placeholder", "event_id": "{event_id}"}})'
        return f'EventType::new("{event_id}", "{description}", {placeholder})'

    # Replace all occurrences
    content = re.sub(pattern, replacement, content)

    if content != original:
        with open(filepath, 'w') as f:
            f.write(content)
        return True
    return False

def main():
    """Process all Rust files in src/ directory."""
    modified_files = []

    for root, dirs, files in os.walk('src'):
        # Skip hidden directories
        dirs[:] = [d for d in dirs if not d.startswith('.')]

        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                if process_file(filepath):
                    modified_files.append(filepath)
                    print(f"Modified: {filepath}")

    # Also process tests directory
    for root, dirs, files in os.walk('tests'):
        dirs[:] = [d for d in dirs if not d.startswith('.')]

        for file in files:
            if file.endswith('.rs'):
                filepath = os.path.join(root, file)
                if process_file(filepath):
                    modified_files.append(filepath)
                    print(f"Modified: {filepath}")

    print(f"\nTotal modified files: {len(modified_files)}")

if __name__ == "__main__":
    main()
