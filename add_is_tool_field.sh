#!/bin/bash

# Script to add is_tool: false to all ActionDefinition structs that don't have it

# Find all Rust files with ActionDefinition
files=$(grep -rl "ActionDefinition {" src/ --include="*.rs")

for file in $files; do
    echo "Processing $file..."

    # Use perl to add is_tool: false, before the closing } of ActionDefinition
    # This looks for patterns like:
    #     }),
    # }
    # and adds is_tool: false, before the closing }

    perl -i -pe '
        # Track if we are inside ActionDefinition
        if (/^\s*ActionDefinition\s*\{/) {
            $in_action_def = 1;
        }

        # If we find a closing brace that closes ActionDefinition
        # and the previous line has }), (closing example field)
        # then add is_tool: false,
        if ($in_action_def && /^\s*\}\s*$/ && $prev_line =~ /\}\),\s*$/ && $prev_line !~ /is_tool:/) {
            # Insert is_tool: false, before this line
            print "        is_tool: false,\n";
            $in_action_def = 0;
        }

        $prev_line = $_;
    ' "$file"
done

echo "Done!"
