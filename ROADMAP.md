# xngmcp roadmap

## Outcome

xngmcp will give coding agents a small, dependable web capability backed by a
private SearXNG instance. The same binary will work in three contexts:

- as a stdio MCP server for Pi, thndrs, and other MCP clients;
- as a human-friendly command-line tool for testing and direct use; and
- as a machine-friendly CLI with stable JSON output for scripts.

Version 0.1 is complete when Pi and thndrs can discover and call `web_search`
and `web_fetch`, both tools return bounded structured results, and the Docker
stack in this repository is serving the machine's local SearXNG endpoint.

## Product boundary

### In scope for 0.1

- A single Rust crate producing a binary named `xngmcp`, with commands defined
  by Clap's derive API.
- Human output styled sparingly with `owo-colors`.
- Structured diagnostic logging to stderr with `tracing` and
  `tracing-subscriber`.
- A Tokio-based stdio MCP server built with the official Rust MCP SDK, `rmcp`.
- A normalized SearXNG search client.
- A public-web fetcher that extracts readable text or Markdown and resists
  server-side request forgery (SSRF).
- A self-contained `infra/searxng` Docker Compose stack using SearXNG and
  Valkey, bound to loopback by default.
- Setup and verification examples for Pi and thndrs.

### Deliberately out of scope

- An MCP HTTP transport, hosted service, authentication, or multi-user quotas.
- Browser automation, JavaScript rendering, PDF parsing, image search, and file
  downloads.
- Tool variants such as `search_news`, `search_images`, or `get_engines`.
- MCP resources, prompts, and a capabilities resource.
- A cache in the Rust process; SearXNG and Valkey own search-side caching.
- Changes to Pi or thndrs themselves.

`web_search_many` is a possible 0.2 addition only if real agent use shows that
the extra tool and concurrency policy reduce turns enough to justify them.

## Interfaces and behavior

The product uses generic web vocabulary. Agents should not need to know that
SearXNG is the current search implementation.

### MCP tools

#### `web_search`

Input:

```json
{
    "query": "Rust MCP SDK structured content",
    "limit": 8,
    "page": 1,
    "language": "all",
    "time_range": "month",
    "categories": ["general"],
    "engines": ["brave", "duckduckgo"],
    "safe_search": 1,
    "include_domains": ["github.com"],
    "exclude_domains": ["example.com"]
}
```

Only `query` is required. Defaults and bounds:

| Field         | Default       | Validation                                |
| ------------- | ------------- | ----------------------------------------- |
| `query`       | none          | non-blank, at most 1,000 characters       |
| `limit`       | `8`           | 1–20                                      |
| `page`        | `1`           | 1–10                                      |
| `language`    | `all`         | non-blank SearXNG language code or `all`  |
| `time_range`  | omitted       | `day`, `month`, or `year`                 |
| `categories`  | `["general"]` | at most 10 non-blank values               |
| `engines`     | omitted       | at most 10 non-blank values               |
| `safe_search` | `1`           | `0`, `1`, or `2`                          |
| domain lists  | omitted       | normalized hostnames, at most 20 per list |

The query is passed through unchanged so SearXNG search syntax remains
available. Include and exclude filters are applied to parsed result hostnames
using exact-or-subdomain matching; filtering does not rely on every engine
understanding `site:` syntax. A filtered page may therefore contain fewer than
`limit` results.

Output:

```json
{
    "query": "Rust MCP SDK structured content",
    "results": [
        {
            "title": "Model Context Protocol Rust SDK",
            "url": "https://github.com/modelcontextprotocol/rust-sdk",
            "snippet": "The official Rust SDK for MCP.",
            "score": 1.0,
            "published_at": "2026-08-01T00:00:00Z",
            "engines": ["brave"]
        }
    ]
}
```

Results are deduplicated by normalized URL while preserving SearXNG rank.
`published_at`, `suggestions`, and `unresponsive_engines` are omitted when the
upstream response supplies no useful value. Invalid upstream result URLs are
dropped rather than handed to the agent.

#### `web_fetch`

Input:

```json
{
    "url": "https://example.com/article",
    "max_chars": 30000,
    "format": "markdown"
}
```

Only `url` is required. `max_chars` defaults to 30,000 and is limited to
1,000–100,000. `format` is `markdown` by default and also accepts `text`.

Output:

```json
{
    "url": "https://example.com/article",
    "title": "Article title",
    "content": "# Article title\n\nReadable content...",
    "content_type": "text/html; charset=utf-8",
    "truncated": false
}
```

Fetch accepts only HTTP and HTTPS. HTML is decoded to UTF-8 and passed to
Lectito, which extracts the readable article, Markdown or plain text, title,
and available metadata. Plain-text responses pass through directly. Binary and
unsupported media types return an actionable tool error. An HTML page that
Lectito determines has no readable article returns an extraction error rather
than silently returning the page chrome.

The fetcher uses a dedicated network client that:

- rejects loopback, private, link-local, unspecified, multicast, and metadata
  service addresses after DNS resolution;
- dials the validated address so DNS rebinding cannot redirect the connection;
- repeats validation for every redirect and caps redirect count;
- applies connect, response-header, and whole-operation timeouts;
- limits the decompressed response body before parsing; and
- never sends credentials or cookies.

The configured SearXNG client is separate and is allowed to reach the expected
loopback service. This distinction prevents the fetch policy from either
blocking the backend or accidentally granting public tool calls access to it.

### CLI signature

```text
xngmcp [--searxng-url URL] [--log-level LEVEL] [--no-color] <command>

xngmcp search [--limit N] [--page N] [--language CODE]
               [--time-range RANGE] [--category NAME]...
               [--engine NAME]... [--safe-search LEVEL]
               [--include-domain HOST]... [--exclude-domain HOST]...
               [--json | --plain] <query>

xngmcp fetch [--max-chars N] [--format markdown|text]
              [--json] <url>

xngmcp serve
xngmcp --version
```

Repeated flags map directly to list fields in the MCP schemas. Search and fetch
call the same application services as the MCP handlers. Each feature owns its
defaults, validation, filtering or truncation rules, and error mapping across
both interfaces.

The interface follows the [Command Line Interface Guidelines](https://clig.dev/):

- Running `xngmcp` with no command shows concise help and examples; `-h`,
  `--help`, and `help <command>` show complete help without making a network
  request.
- Search terms and URLs are required positional arguments; optional behavior is
  expressed with long, unambiguous flags. Clap's typo suggestions are allowed,
  but arbitrary subcommand abbreviations are not.
- Primary results go to stdout. Errors, TTY-only status messages, and `tracing`
  diagnostics go to stderr. MCP protocol messages are the only bytes ever
  written to stdout by `serve`.
- Human output is brief and scannable. `search --plain` emits one tab-separated
  result per line without wrapping or styling; embedded tabs and newlines are
  collapsed. `--json` emits the same stable objects used by MCP, with no
  decoration. JSON remains JSON even on a TTY.
- `owo-colors` styling is disabled when the relevant stream is not a TTY, when
  `NO_COLOR` is non-empty, when `TERM=dumb`, or when `--no-color` is set.
- A TTY-only status line appears on stderr before a network wait and is removed
  when the command completes. Non-TTY execution never emits animation.
- Expected failures are rewritten as concise, actionable messages without stack
  traces. Exit status is `0` for success, `2` for usage or validation errors,
  and `1` for backend, network, extraction, or internal errors.
- SIGINT and SIGTERM cancel the root cancellation token promptly. Every network
  operation has a reasonable default timeout, overridable through the documented
  environment variables.
- Flags override environment variables, which override defaults. Stable
  machine-local settings use environment variables rather than an implicit
  config file.

Environment variables:

| Variable                | Default                 | Purpose                |
| ----------------------- | ----------------------- | ---------------------- |
| `SEARXNG_URL`           | `http://127.0.0.1:8080` | Search backend URL     |
| `XNGMCP_SEARCH_TIMEOUT` | `15s`                   | Whole search deadline  |
| `XNGMCP_FETCH_TIMEOUT`  | `20s`                   | Whole fetch deadline   |
| `XNGMCP_LOG_LEVEL`      | `warn`                  | Diagnostic verbosity   |
| `NO_COLOR`              | unset                   | Standard color opt-out |

No query text, fetched content, credentials, or full URL query string is logged
at the default level.

## Architecture

`src/main.rs` delegates to `xngmcp::run()` in `src/lib.rs`. The feature flow
below begins after that library entry point. Separately, `build.rs` consumes the
shared Clap definition and writes man-page and completion artifacts to
`OUT_DIR`.

```text
src/cli.rs (Clap) ──────────────────┐
                                    v
src/mcp.rs (rmcp) ───────────> src/web/
                                  ├── search service
                                  │     └── SearXNG client
                                  └── fetch service
                                        ├── public-only HTTP client
                                        └── Lectito extraction

src/output.rs
  └── owo-colors human/plain/JSON renderers and stderr status
```

The planned repository shape is:

```text
Cargo.toml
Cargo.lock
build.rs
src/
  lib.rs
  main.rs
  cli.rs
  config.rs
  error.rs
  mcp.rs
  output.rs
  searxng.rs
  web/
    mod.rs
    fetch.rs
    search.rs
tests/
infra/searxng/
  .env.example
  bootstrap.sh
  compose.yaml
  settings.yml
  README.md
```

This is one Cargo package with one library target and one thin binary target,
not a workspace. `src/main.rs` contains only the Tokio entry point and returns
the result of `xngmcp::run()`. The public asynchronous `run()` function in
`src/lib.rs` owns argument parsing, configuration, tracing setup, cancellation,
command dispatch, and process exit status. Keeping that boundary in the library
makes the application flow callable from integration tests without duplicating
the binary entry point.

`src/cli.rs` owns only Clap parser types and command metadata; it must not depend
on service modules or perform I/O. `src/lib.rs` maps parsed commands to feature
services. `build.rs` reuses the same parser module with a path module and Clap's
`CommandFactory`, then uses `clap_mangen::generate_to` and `clap_complete` to
write the root and subcommand man pages plus Bash, Zsh, Fish, Elvish, and
PowerShell completions under `OUT_DIR`. It declares `src/cli.rs` and
`Cargo.toml` as rerun inputs and exposes the generated directory to tests with a
Cargo compile-time environment value. Generated files never modify the source
tree.

Core request and result types live with their feature modules under `src/web`.
Traits are introduced only at network boundaries where tests need substitution;
single-use helpers stay inline. The SearXNG adapter owns query parameter mapping
and upstream normalization. The MCP and CLI modules own transport concerns but
no search or fetch logic.

Direct dependencies are limited to what the implementation uses: `rmcp`,
`clap`, `owo-colors`, `tracing`, `tracing-subscriber`, `lectito`, Tokio,
`reqwest`, `serde`, `serde_json`, `schemars`, URL and charset handling, and a
small error or cancellation utility where the standard library is insufficient.
Build dependencies are `clap`, `clap_mangen`, and `clap_complete`; the command
definition is shared rather than reconstructed in `build.rs`.

Use released crates from crates.io, set a supported `rust-version`, and commit
`Cargo.lock` because this repository ships an application. Do not use path
dependencies or floating Git branches; the project must build without another
local checkout.

## Delivery plan

### Milestone 1: application shell and local infrastructure

Build the library-backed Clap application shell, configuration loader,
cancellation token, version output, generated man page and completions, and
`tracing` subscriber. In parallel, create the self-contained `infra/searxng`
stack so search and CLI development can use a real backend from the start.

Compose runs pinned SearXNG and Valkey images, binds SearXNG to
`127.0.0.1:${SEARXNG_PORT:-8080}`, mounts settings read-only, uses health
checks, and keeps only a disposable named cache volume. `settings.yml` enables
JSON output, keeps the instance private, disables the public limiter and image
proxy, uses safe search level 1, connects to Valkey, and applies outgoing
timeouts. `bootstrap.sh` creates a mode-0600 `.env` with a fresh random secret
without overwriting an existing file.

Acceptance:

- Root help, version output, configuration precedence, stderr logging, and
  signal cancellation work without a running backend.
- A clean build generates man pages and completions from the same command
  definition used at runtime, and tests detect stale or missing artifacts.
- `docker compose config` succeeds without exposing the generated secret.
- Both containers become healthy after a clean start and restart.
- SearXNG is unreachable through non-loopback host interfaces by default.
- `/healthz` and a JSON `/search` request succeed on the development port.

### Milestone 2: web features and CLI

Build the normalized search service, domain filtering, and hardened fetch path.
Add `search` and `fetch` Clap subcommands over those services. Use `owo-colors`
only for TTY presentation and `tracing` only for stderr diagnostics. Use Lectito
for readable HTML extraction and its Markdown/plain-text output. Exercise search
against the Milestone 1 stack and fetch one returned public URL.

Acceptance:

- Search tests cover parameter encoding, malformed responses, deduplication,
  ordering, domain boundary matching, validation limits, and a real request to
  the local stack.
- Fetch tests cover public hosts, blocked IPv4 and IPv6 ranges, DNS resolution,
  redirect-to-private attempts, unsupported media, oversized decompressed
  bodies, charset conversion, extraction, and Unicode-safe truncation.
- CLI tests cover help, exact argument counts, repeated flags, `--plain`,
  `--json`, no-color conditions, stream separation, cancellation, and exit
  codes.
- A CLI search against the local stack followed by a fetch of one result
  succeeds in human and JSON modes.

### Milestone 3: MCP stdio server

Register typed `web_search` and `web_fetch` handlers with `rmcp`. Derive
`serde::Deserialize` and `schemars::JsonSchema` on each feature's input type so
the SDK's tool macros expose JSON schemas from the same types the services
validate. Return structured content with a compatible text fallback. Run the
server on Tokio over rmcp's stdio transport and route logs exclusively to
stderr.

Acceptance:

- An in-process MCP client lists exactly the two intended tools with useful
  agent-oriented descriptions and complete schemas.
- Valid calls return structured content matching the CLI JSON shape.
- Invalid input and expected backend failures are tool errors rather than MCP
  protocol failures.
- A subprocess test proves stdout contains only valid MCP protocol frames while
  logging is enabled.
- Closing stdin or sending a cancellation signal stops the server cleanly.

### Milestone 4: agent integration and release readiness

Cut over only after the CLI and MCP server pass against the staged stack:

1. Run the stack on `127.0.0.1:18080` and verify health, JSON search, CLI search
   and fetch, and both MCP tools.
2. Record how to stop and restart the service currently using port 8080.
3. Stop that service, restart the validated xngmcp stack on port 8080, and
   repeat the checks.
4. Exercise rollback before retiring the prior deployment configuration. No
   data migration is required because Valkey contains disposable cache data.

Document local installation and add copyable MCP configurations. Pi uses its
installed MCP adapter with a project `.mcp.json` entry:

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

thndrs uses a project `.thndrs/mcp.toml` entry:

```toml
[servers.web]
transport = "stdio"
command = "xngmcp"
args = ["serve"]
env = { SEARXNG_URL = "http://127.0.0.1:8080" }
enabled = true
timeout_secs = 30
```

The thndrs setup notes include its required project trust step. Documentation
also explains that the binary must be on the agent process's `PATH`, SearXNG
must already be healthy, and the tools may be presented with client-specific
names or through Pi's lazy MCP proxy.

Acceptance:

- The repository-owned stack serves port 8080 after restart, and the tested
  rollback restores the prior local service.
- Pi discovers the server, searches for a current topic, fetches one returned
  public URL, and receives bounded content without native web-search support.
- thndrs passes `mcp test web`, lists both tools, and completes the same
  search-then-fetch flow after project trust is granted.
- `cargo fmt --all -- --check`, `cargo check --all-targets --all-features`,
  `cargo clippy --all-targets --all-features -- -D warnings`,
  `cargo test --all-features`, and Docker configuration validation pass.
- The README contains the shortest install/start/integrate path; detailed
  operational guidance remains in the infra README and command help.

## Risks and controls

| Risk                                    | Control                                                                                      |
| --------------------------------------- | -------------------------------------------------------------------------------------------- |
| Fetched URLs reach local services       | Public-only resolver/dialer, redirect revalidation, and table-driven SSRF tests              |
| MCP stdout is corrupted by UI or logs   | Separate writers; subprocess protocol test with verbose logging                              |
| Search engines return inconsistent data | Narrow adapter, tolerant optional fields, invalid-URL filtering, stable normalized types     |
| Agent context is flooded                | Search limit of 20, fetch character cap, body byte cap, and concise tool descriptions        |
| CLI and MCP behavior diverge            | Each feature shares types, validation, and services across interfaces and tests both outputs |
| SearXNG upgrade breaks JSON search      | Immutable image pins, explicit JSON setting, health and real search smoke tests              |
| Port-8080 cutover causes downtime       | Parallel validation on 18080 and a documented rollback to the previous service configuration |
| Terminal decoration breaks scripts      | Explicit `--json`/`--plain`, TTY detection, `NO_COLOR`, and stdout/stderr tests              |

## References

- [Official Rust MCP SDK (`rmcp`)](https://github.com/modelcontextprotocol/rust-sdk)
- [Lectito](https://github.com/stormlightlabs/lectito)
- [Clap](https://github.com/clap-rs/clap)
- [clap_mangen](https://docs.rs/clap_mangen/)
- [clap_complete](https://docs.rs/clap_complete/)
- [Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html)
- [owo-colors](https://github.com/jam1garner/owo-colors)
- [tracing](https://github.com/tokio-rs/tracing)
- [Command Line Interface Guidelines](https://clig.dev/)
- [SearXNG search API](https://docs.searxng.org/dev/search_api.html)
- [SearXNG container installation](https://docs.searxng.org/admin/installation-docker.html)
- [SearXNG settings](https://docs.searxng.org/admin/settings/settings.html)
