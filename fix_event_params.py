#!/usr/bin/env python3
"""Convert EventType parameters from ParameterDefinition to Parameter"""

import re
import sys

def fix_event_type_parameters(content):
    """Convert ParameterDefinition to Parameter in EventType.with_parameters() calls"""

    # Pattern to match .with_parameters(vec![...]) blocks in EventType chains
    # We need to find these within EventType::new(...).with_parameters(...) chains

    def convert_param_def_to_param(param_block):
        """Convert a single ParameterDefinition block to Parameter"""
        # Remove the example field and trailing comma before closing brace
        param_block = re.sub(r',?\s*example:\s*json!\([^)]*\),?\s*', '', param_block)
        # Change ParameterDefinition to Parameter
        param_block = param_block.replace('ParameterDefinition {', 'Parameter {')
        return param_block

    # Find all with_parameters blocks that are part of EventType chains
    # Pattern: .with_parameters(vec![...])
    pattern = r'(\.with_parameters\(vec!\[)(.*?)(\]\))'

    def fix_params_block(match):
        prefix = match.group(1)
        params_content = match.group(2)
        suffix = match.group(3)

        # Convert all ParameterDefinition to Parameter within this block
        fixed_params = convert_param_def_to_param(params_content)

        return prefix + fixed_params + suffix

    return re.sub(pattern, fix_params_block, content, flags=re.DOTALL)

def main():
    if len(sys.argv) != 2:
        print("Usage: fix_event_params.py <file>")
        sys.exit(1)

    filepath = sys.argv[1]

    with open(filepath, 'r') as f:
        content = f.read()

    fixed_content = fix_event_type_parameters(content)

    if content != fixed_content:
        with open(filepath, 'w') as f:
            f.write(fixed_content)
        print(f"Fixed EventType parameters in {filepath}")
        return 0
    else:
        print(f"No EventType parameter changes needed for {filepath}")
        return 0

if __name__ == '__main__':
    sys.exit(main())
