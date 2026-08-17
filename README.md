# xngmcp

xngmcp gives MCP clients and the terminal two local-web tools: `web_search` via
SearXNG and `web_fetch` for readable public pages.

## Install

Requires Rust 1.85 or newer. Docker with the Compose plugin is required for the
included SearXNG backend.

From a source checkout, install xngmcp and start the backend:

```sh
cargo install --path .

./infra/searxng/bootstrap.sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

The backend listens on `http://127.0.0.1:8080`. Confirm it and the CLI work:

```sh
curl --fail --silent --show-error http://127.0.0.1:8080/healthz >/dev/null
xngmcp search --limit 5 'Rust MCP SDK'
```

See [the SearXNG guide](infra/searxng/README.md) for configuration, operations,
and its integration test. See [shell integration](docs/shell-integration.md) to
install the generated man pages and shell completions.

## Pi

Install and configure the Pi package with its
[setup guide](packages/pi-plugin-xngmcp/README.md). It configures
`pi-mcp-adapter` to run `xngmcp serve` and exposes `web_search` and `web_fetch`
as direct Pi tools.

## Usage

```sh
# Search the web
xngmcp search 'Rust MCP SDK'
xngmcp search --json --limit 5 'Rust MCP SDK'

# Fetch readable text or Markdown from a public URL
xngmcp fetch --format text https://doc.rust-lang.org/book/

# Run the stdio MCP server
xngmcp serve
```

Configure an MCP client to start `xngmcp serve` with
`SEARXNG_URL=http://127.0.0.1:8080`. The server exposes `web_search` and
`web_fetch`. Use `--searxng-url` or `SEARXNG_URL` to select another endpoint.

`search --json` and `fetch --json` write a single JSON value to stdout. Errors
and diagnostics go to stderr. Run `xngmcp --help` for all commands and options.
