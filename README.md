# xngmcp

xngmcp searches a local SearXNG instance from the terminal or through an MCP
server. It is a Rust application under active development.

Today, the CLI provides `search` and `fetch`. The MCP server provides
`web_search`; `web_fetch` is not available over MCP yet.

## Requirements

- Rust 1.85 or newer
- Docker with the Compose plugin, to run the included SearXNG stack

## Start locally

From the repository root, build the binary and start SearXNG:

```sh
cargo build --release
./infra/searxng/bootstrap.sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

The stack listens on `http://127.0.0.1:8080` by default. Check it before
using xngmcp:

```sh
curl --fail --silent --show-error http://127.0.0.1:8080/healthz
```

See [infra/searxng/README.md](infra/searxng/README.md) for configuration,
logs, port changes, upgrades, and removal.

## Use the CLI

Run the release binary directly:

```sh
./target/release/xngmcp search 'Rust MCP SDK'
./target/release/xngmcp search --json --limit 5 'Rust MCP SDK'
./target/release/xngmcp search --include-domain github.com --plain 'rmcp'
./target/release/xngmcp fetch --format text https://example.com/article
```

`search --json` and `fetch --json` write one JSON value to stdout. Search
output in `--plain` mode has one tab-separated title, URL, and snippet per
line. Errors and diagnostics go to stderr.

Use `--searxng-url` for a non-default endpoint, or set it once for the shell:

```sh
SEARXNG_URL=http://127.0.0.1:18080 \
  ./target/release/xngmcp search --json 'Rust MCP SDK'
```

Run `xngmcp --help`, `xngmcp search --help`, or `xngmcp fetch --help` for the
full option list. The search command accepts a query and optional result/page
limits, language, time range, repeated categories and engines, safe-search
level, and repeated include/exclude domain filters.

## Use `web_search` through MCP

Install the binary where the MCP client can find it:

```sh
cargo install --path .
```

Add this stdio server to an MCP client configuration. The configuration shape
below is used by clients that accept `mcpServers`:

```json
{
  "mcpServers": {
    "web": {
      "command": "xngmcp",
      "args": ["serve"],
      "env": {
        "SEARXNG_URL": "http://127.0.0.1:8080"
      }
    }
  }
}
```

Restart or reload the client, then inspect its tool list. It should contain
`web_search`. The tool accepts `query` plus the same optional search controls
as the CLI: `limit` (1–20, default 8), `page` (1–10, default 1), `language`,
`time_range`, `categories`, `engines`, `safe_search` (0–2, default 1), and
include/exclude domain lists. It returns structured search results and a short
text summary.

`xngmcp serve` uses stdin and stdout for MCP protocol messages. Do not run it
interactively or write other output to its stdout. Set `XNGMCP_LOG_LEVEL=debug`
when diagnosing a client launch or backend problem; diagnostics remain on
stderr.

## Common problems

- **SearXNG cannot be reached:** run the health check above, then confirm the
  `SEARXNG_URL` seen by xngmcp points to that loopback address and port.
- **The MCP client cannot start xngmcp:** install it with `cargo install --path
  .` and make sure the client process has Cargo's bin directory on `PATH`.
- **`web_fetch` is missing:** it is not part of the MCP server in the current
  version. Use `xngmcp fetch` from the terminal.

## Checks

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```
