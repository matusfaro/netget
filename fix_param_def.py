#!/usr/bin/env python3
"""Fix ParameterDefinition by adding missing 'example' field"""

import re
import sys

def fix_parameter_definition(content):
    """Add example field to ParameterDefinition instances that are missing it"""

    # Pattern to match ParameterDefinition blocks
    # Matches from 'ParameterDefinition {' to the closing '}'
    pattern = r'(ParameterDefinition\s*\{[^}]*?required:\s*(?:true|false),?\s*)\}'

    def add_example(match):
        block = match.group(1)
        # Check if example is already present
        if 'example:' in block:
            return block + '}'

        # Extract the parameter name to create a sensible example
        name_match = re.search(r'name:\s*"([^"]+)"', block)
        type_match = re.search(r'type_hint:\s*"([^"]+)"', block)

        param_name = name_match.group(1) if name_match else "value"
        param_type = type_match.group(1) if type_match else "string"

        # Create appropriate example based on type
        if param_type == "number":
            example = 'json!(100)'
        elif param_type == "boolean":
            example = 'json!(true)'
        elif param_type == "array":
            example = 'json!([])'
        elif param_type == "object":
            example = 'json!({})'
        else:  # string or default
            example = f'json!("{param_name}")'

        # Add example field with proper indentation
        # Find the indentation of the last field
        lines = block.split('\n')
        if len(lines) > 1:
            # Get indentation from the 'required' line
            for line in reversed(lines):
                if 'required:' in line:
                    indent = len(line) - len(line.lstrip())
                    break
            else:
                indent = 20  # default
        else:
            indent = 20

        indent_str = ' ' * indent
        return block + f'\n{indent_str}example: {example},\n{indent_str[:-4]}}}'

    return re.sub(pattern, add_example, content, flags=re.DOTALL)

def main():
    if len(sys.argv) != 2:
        print("Usage: fix_param_def.py <file>")
        sys.exit(1)

    filepath = sys.argv[1]

    with open(filepath, 'r') as f:
        content = f.read()

    fixed_content = fix_parameter_definition(content)

    if content != fixed_content:
        with open(filepath, 'w') as f:
            f.write(fixed_content)
        print(f"Fixed {filepath}")
        return 0
    else:
        print(f"No changes needed for {filepath}")
        return 0

if __name__ == '__main__':
    sys.exit(main())
