# @smotana/netget

LLM-controlled network protocol server & client — 50+ protocols (HTTP, DNS, SSH, MySQL, Redis, WireGuard, …) driven by an LLM, with a built-in [MCP](https://modelcontextprotocol.io) server mode.

This package is a small launcher that runs the platform-native `netget` binary. The binary itself is installed via a platform-specific optional dependency (e.g. `@smotana/netget-darwin-arm64`); if that is unavailable, the launcher downloads the matching binary from [GitHub Releases](https://github.com/smotanacom/netget/releases) into your user cache.

## Quick start (MCP server)

```bash
npx @smotana/netget --mcp
```

Add to Claude Code:

```bash
claude mcp add netget -- npx -y @smotana/netget --mcp
```

Or in `claude_desktop_config.json` / `.mcp.json`:

```json
{
  "mcpServers": {
    "netget": {
      "command": "npx",
      "args": ["-y", "@smotana/netget", "--mcp"]
    }
  }
}
```

## Runtime requirements

NetGet needs an LLM backend at runtime: a local [Ollama](https://ollama.ai) (default, `http://localhost:11434`) or any OpenAI-compatible endpoint via `--openai-url`, `--model`, and `--api-key` (or `NETGET_API_KEY`).

## Interactive TUI

```bash
npx @smotana/netget
```

## Notes

- Prebuilt binaries use a portable feature set (no BLE/NFC/packet-capture on Linux, no SMB client). Build from source for `all-protocols`: https://github.com/smotanacom/netget
- Env overrides: `NETGET_BINARY` (use a specific binary), `NETGET_DOWNLOAD_BASE` (alternate download mirror).

## License

AGPL-3.0-or-later. Source: https://github.com/smotanacom/netget
