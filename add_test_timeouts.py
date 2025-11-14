#!/usr/bin/env python3
"""
Script to add timeout wrappers to test functions in NetGet E2E tests.
This prevents tests from hanging indefinitely during parallel execution.
"""

import re
import sys
from pathlib import Path

def add_timeout_to_test_file(file_path: Path) -> bool:
    """
    Add timeout wrappers to all test functions in a file.
    Returns True if file was modified, False otherwise.
    """
    content = file_path.read_text()
    original_content = content

    # Check if with_timeout is already imported
    if 'with_timeout' not in content:
        # Add with_timeout to the imports
        import_pattern = r'(use crate::helpers::\{[^}]*)(E2EResult[^}]*)\};'
        if re.search(import_pattern, content):
            content = re.sub(
                import_pattern,
                r'\1E2EResult, with_timeout\2};',
                content
            )
        else:
            # Try alternative import pattern
            import_pattern2 = r'(use crate::helpers::(?:server::)?(?:helpers::)?)\{([^}]*)\}'
            if re.search(import_pattern2, content):
                content = re.sub(
                    import_pattern2,
                    r'\1{\2, with_timeout}',
                    content
                )
            else:
                # Add as new import line after existing helper imports
                helper_import = r'(use crate::(?:helpers|server::helpers)[^;]*;)'
                if re.search(helper_import, content):
                    content = re.sub(
                        helper_import,
                        r'\1\nuse crate::helpers::with_timeout;',
                        content,
                        count=1
                    )

    # Check if std::time::Duration is imported (needed for timeout duration)
    if 'use std::time::Duration' not in content:
        # Add Duration import after existing use statements
        use_section = r'((?:use [^;]+;\n)+)'
        if re.search(use_section, content):
            content = re.sub(
                use_section,
                r'\1use std::time::Duration;\n',
                content,
                count=1
            )

    # Find all test functions and wrap them
    # Pattern: #[tokio::test]\nasync fn test_name() -> E2EResult<()> {\n    // body\n}
    test_pattern = r'(#\[tokio::test\]\s*\n\s*async fn (test_\w+)\(\) -> E2EResult<\(\)> \{)\n((?:(?!^async fn |^fn |^\}$).)*)'

    def wrap_test(match):
        """Wrap test body with timeout"""
        test_header = match.group(1)
        test_name = match.group(2)
        test_body = match.group(3)

        # Skip if already wrapped
        if 'with_timeout' in test_body[:200]:  # Check first 200 chars
            return match.group(0)

        # Extract first line of body (usually whitespace)
        lines = test_body.split('\n')
        indent = '    '  # Standard 4-space indent

        # Build wrapped version
        wrapped = f'''{test_header}
{indent}with_timeout("{test_name}", Duration::from_secs(120), async {{
{test_body}
{indent}}}).await
}}'''

        return wrapped

    # Apply wrapping
    modified_content = content
    test_functions = re.finditer(
        r'#\[tokio::test\]\s*\n\s*async fn (test_\w+)\(\) -> E2EResult<\(\)> \{',
        content
    )

    # Process tests in reverse order to preserve positions
    tests = list(test_functions)
    for match in reversed(tests):
        test_name = match.group(1)
        start_pos = match.start()

        # Find the matching closing brace
        brace_count = 0
        pos = match.end()
        found_end = False

        while pos < len(modified_content):
            char = modified_content[pos]
            if char == '{':
                brace_count += 1
            elif char == '}':
                if brace_count == 0:
                    found_end = True
                    break
                brace_count -= 1
            pos += 1

        if not found_end:
            continue

        # Extract test function
        test_func = modified_content[start_pos:pos+1]

        # Check if already wrapped
        if 'with_timeout' in test_func[:500]:
            continue

        # Parse the function
        header_end = test_func.find('{')
        header = test_func[:header_end+1]
        body = test_func[header_end+1:-1]  # Remove opening and closing braces

        # Wrap the body
        wrapped_func = f'''{header}
    with_timeout("{test_name}", Duration::from_secs(120), async {{
{body}
    }}).await
}}'''

        # Replace in content
        modified_content = modified_content[:start_pos] + wrapped_func + modified_content[pos+1:]

    # Only write if modified
    if modified_content != original_content:
        file_path.write_text(modified_content)
        print(f"✓ Modified: {file_path}")
        return True
    else:
        print(f"  Skipped: {file_path} (no changes needed)")
        return False


def main():
    """Process all test files that need timeout wrappers."""

    # List of protocols with hanging tests (from GROUP 2)
    hanging_protocols = [
        "bluetooth_ble_beacon",
        "bluetooth_ble_data_stream",
        "bluetooth_ble_environmental",
        "bluetooth_ble_file_transfer",
        "cassandra",
        "dynamo",
        "grpc",
        "http",
        "ldap",
        "openapi",
        "rip",
        "smb",
    ]

    project_root = Path(__file__).parent
    tests_dir = project_root / "tests" / "server"

    if not tests_dir.exists():
        print(f"Error: Tests directory not found: {tests_dir}")
        return 1

    modified_count = 0

    for protocol in hanging_protocols:
        protocol_dir = tests_dir / protocol
        if not protocol_dir.exists():
            print(f"Warning: Protocol directory not found: {protocol_dir}")
            continue

        # Find all e2e test files
        test_files = list(protocol_dir.glob("e2e*.rs"))

        for test_file in test_files:
            if add_timeout_to_test_file(test_file):
                modified_count += 1

    print(f"\n✓ Modified {modified_count} test files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
