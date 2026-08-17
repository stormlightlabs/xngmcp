# xngmcp for Pi

This package gives Pi guidance for xngmcp's `web_search` and `web_fetch` MCP
tools. It does not run a web client or an MCP server. `pi-mcp-adapter` starts
`xngmcp serve` and exposes the two tools directly in Pi.

## Install

From an xngmcp source checkout, install the binary and start the local SearXNG
stack:

```sh
cargo install --path .
./infra/searxng/bootstrap.sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

Install the adapter and this package:

```sh
pi install npm:pi-mcp-adapter
pi install npm:pi-plugin-xngmcp
```

Check that the binary and backend are ready:

```sh
command -v xngmcp
curl --fail --silent --show-error http://127.0.0.1:8080/healthz >/dev/null
```

## Configure a project

Create `.mcp.json` in the project root with this configuration. The package
also includes it as `examples/.mcp.json`:

```json
{
  "mcpServers": {
    "web": {
      "command": "xngmcp",
      "args": ["serve"],
      "env": {
        "SEARXNG_URL": "http://127.0.0.1:8080"
      },
      "directTools": ["web_search", "web_fetch"],
      "toolPrefix": "none"
    }
  }
}
```

`toolPrefix: "none"` preserves the MCP names, so Pi shows `web_search` and
`web_fetch` beside its built-in tools. The adapter is otherwise responsible for
MCP discovery, connection management, and diagnostics.

Start Pi from the configured project. On a new Pi agent directory, populate the
adapter cache and reload the session:

```text
/mcp reconnect web
/reload
```

Open `/mcp` to confirm that `web` is connected and both tools are direct. Ask a
normal current-information question. Pi should search, select a relevant public
result, and fetch it with a proportionate `max_chars` limit.

## Diagnose a failed setup

| Symptom | Check |
| --- | --- |
| Package is absent | `pi list`, then install `pi-mcp-adapter` and `pi-plugin-xngmcp`. |
| `xngmcp` will not start | Run `command -v xngmcp`; put the installed binary on `PATH`. |
| Tools are missing or stale | Run `/mcp reconnect web`, then `/reload`. |
| Server launch failed | Open `/mcp`. Set `"debug": true` on `web` temporarily, reconnect, and inspect the displayed stderr. |
| Search backend cannot be reached | Run the health check, verify `SEARXNG_URL`, and reconnect. Pi reports this as a tool error and remains usable. |

Use the `/mcp` panel for connection and tool diagnostics rather than adding a
second xngmcp interface.
