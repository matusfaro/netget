#!/bin/bash

# Fix inline ActionDefinition that aren't followed by closing } on same line

for file in src/server/proxy/actions.rs src/server/bgp/actions.rs; do
    echo "Processing $file..."

    # Use awk to add is_tool: false, to ActionDefinition literals
    awk '
    /ActionDefinition \{/ {
        in_action_def = 1
        print
        next
    }

    in_action_def && /example: json!\(/ {
        example_line = 1
    }

    in_action_def && example_line && /\}\),/ {
        print
        print "            is_tool: false,"
        example_line = 0
        next
    }

    in_action_def && /^\s*\},?\s*$/ {
        in_action_def = 0
    }

    { print }
    ' "$file" > "$file.tmp" && mv "$file.tmp" "$file"
done

echo "Done!"
