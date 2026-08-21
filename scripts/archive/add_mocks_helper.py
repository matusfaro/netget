#!/usr/bin/env python3
"""
Helper script to analyze test files and generate mock configurations.
This script identifies tests that need mocks and suggests appropriate mock patterns.
"""

import re
import sys
from pathlib import Path
from typing import List, Dict, Tuple

def find_test_functions(file_path: Path) -> List[Dict]:
    """Find all test functions in a file that need mocks."""
    with open(file_path, 'r') as f:
        content = f.read()

    tests = []
    # Find all #[tokio::test] functions
    pattern = r'#\[tokio::test\][^}]*?async fn (\w+)\([^)]*\) -> E2EResult<\(\)> \{([^}]+?\}){1,100}'
    matches = re.finditer(pattern, content, re.DOTALL)

    for match in matches:
        test_name = match.group(1)
        test_body = match.group(2)

        # Check if already has mocks
        has_mock = '.with_mock(' in test_body
        has_verify = '.verify_mocks()' in test_body

        # Extract prompt
        prompt_match = re.search(r'let prompt = "([^"]+)"', test_body)
        prompt = prompt_match.group(1) if prompt_match else None

        # Check if uses ServerConfig or NetGetConfig
        uses_server_config = 'ServerConfig::new(' in test_body
        uses_netget_config = 'NetGetConfig::new(' in test_body

        tests.append({
            'name': test_name,
            'has_mock': has_mock,
            'has_verify': has_verify,
            'prompt': prompt,
            'uses_server_config': uses_server_config,
            'uses_netget_config': uses_netget_config,
            'needs_work': not has_mock or not has_verify
        })

    return tests

def analyze_test_file(file_path: Path):
    """Analyze a test file and print report."""
    print(f"\n=== {file_path} ===")
    tests = find_test_functions(file_path)

    if not tests:
        print("  No test functions found")
        return

    needs_mock = [t for t in tests if not t['has_mock']]
    needs_verify = [t for t in tests if t['has_mock'] and not t['has_verify']]
    complete = [t for t in tests if t['has_mock'] and t['has_verify']]

    print(f"  Total tests: {len(tests)}")
    print(f"  Complete (has mocks + verify): {len(complete)}")
    print(f"  Needs mocks: {len(needs_mock)}")
    print(f"  Needs verify_mocks(): {len(needs_verify)}")

    if needs_mock:
        print(f"\n  Tests needing mocks:")
        for t in needs_mock[:5]:  # Show first 5
            print(f"    - {t['name']}")
            if t['prompt']:
                print(f"      Prompt: {t['prompt'][:80]}...")
        if len(needs_mock) > 5:
            print(f"    ... and {len(needs_mock) - 5} more")

    if needs_verify:
        print(f"\n  Tests needing verify_mocks():")
        for t in needs_verify:
            print(f"    - {t['name']}")

def main():
    if len(sys.argv) < 2:
        print("Usage: python3 add_mocks_helper.py <test_file_or_directory>")
        sys.exit(1)

    path = Path(sys.argv[1])

    if path.is_file():
        analyze_test_file(path)
    elif path.is_dir():
        # Recursively find all test files
        test_files = list(path.rglob("*test.rs")) + list(path.rglob("*_test.rs"))

        for test_file in sorted(test_files):
            analyze_test_file(test_file)

        # Print summary
        print("\n" + "="*80)
        print("SUMMARY")
        print("="*80)
        total_files = len(test_files)
        print(f"Analyzed {total_files} test files")
    else:
        print(f"Error: {path} is not a file or directory")
        sys.exit(1)

if __name__ == "__main__":
    main()
