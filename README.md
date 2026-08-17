# xngmcp

xngmcp gives MCP clients and the terminal two local-web tools: `web_search` via
SearXNG and `web_fetch` for readable public pages.

## Build from source

Requires Rust 1.85 or newer. Docker with the Compose plugin is required when
using the included SearXNG stack.

From a source checkout:

```sh
cargo build --release

./infra/searxng/bootstrap.sh

docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

The stack listens on `http://127.0.0.1:8080`. See
[infra/searxng/README.md](infra/searxng/README.md) for its configuration and
operations.

To make the server available to an MCP client, install the built source package:

```sh
cargo install --path .
```

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
