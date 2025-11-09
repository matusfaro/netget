#!/bin/bash

# Script to detect if running in Claude Code for Web environment
# Checks multiple environment variables in order of reliability

detect_claude_code_web() {
    # Primary detection method
    if [ "$CLAUDE_CODE_REMOTE" = "true" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_REMOTE=true)"
        return 0
    fi

    # Secondary detection method
    if [ "$CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE" = "cloud_default" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_REMOTE_ENVIRONMENT_TYPE=cloud_default)"
        return 0
    fi

    # Tertiary detection methods
    if [ "$CLAUDE_CODE_ENTRYPOINT" = "remote" ]; then
        echo "✓ Running in Claude Code for Web (detected via CLAUDE_CODE_ENTRYPOINT=remote)"
        return 0
    fi

    if [ "$IS_SANDBOX" = "yes" ]; then
        echo "✓ Running in Claude Code for Web (detected via IS_SANDBOX=yes)"
        return 0
    fi

    # Not detected
    echo "✗ Not running in Claude Code for Web (local environment)"
    return 1
}

# Run detection
detect_claude_code_web
exit_code=$?

# Print additional guidance based on result
if [ $exit_code -eq 0 ]; then
    echo ""
    echo "⚠️  IMPORTANT: Use --no-default-features with explicit feature selection"
    echo "   Example: ./cargo-isolated.sh build --no-default-features --features tcp,http,dns"
    echo "   DO NOT use --all-features (includes bluetooth-ble which is unavailable)"
else
    echo ""
    echo "ℹ️  You can use --all-features in local environment"
    echo "   Example: ./cargo-isolated.sh build --all-features"
fi

exit $exit_code
