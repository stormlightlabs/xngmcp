# xngmcp implementation tickets

Implement one ticket per fresh agent context. The interface shapes, limits, and
operational decisions are defined in [ROADMAP.md](ROADMAP.md); each ticket owns
the tests for the behavior it introduces.

## T01 — Establish the application shell

**What to build:** A runnable single-package Rust application whose thin binary
delegates to a testable library entry point, with the shared process behavior
and generated reference material needed by the CLI and stdio server.

**Work:**

- [x] Create one Cargo package with `Cargo.toml`, a committed `Cargo.lock`, a
  documented `rust-version`, one library target, one binary target, no workspace
  members, and no local path dependencies.
- [x] Add `src/lib.rs` with public `async fn run() -> ExitCode`. It owns argument
  parsing, configuration, tracing initialization, cancellation wiring, command
  dispatch, and the mapping from expected failures to process exit status.
- [x] Keep `src/main.rs` to the Tokio entry point that returns
  `xngmcp::run().await`; do not duplicate parsing, logging, or error handling in
  the binary target.
- [x] Add released versions of Clap, `owo-colors`, `tracing`,
  `tracing-subscriber`, Tokio, `tokio-util`, `serde`, and `serde_json`; add every
  other dependency in the ticket that first uses it.
- [x] Define the root parser and `serve` subcommand in a side-effect-free
  `src/cli.rs` with Clap's derive API, and expose the build version through
  `xngmcp --version`. Keep parser types independent of application service
  modules so the build script can compile the same command definition.
- [x] Add `build.rs` and build dependencies on Clap, `clap_mangen`, and
  `clap_complete`. Reuse `src/cli.rs` through a path module and Clap's
  `CommandFactory`; do not maintain a second command tree.
- [x] Use `clap_mangen::generate_to` to generate the root and subcommand man
  pages, and generate static completions for Bash, Zsh, Fish, Elvish, and
  PowerShell into `assets/man` and `assets/completions`. Ignore both generated
  directories in Git, emit rerun directives for `src/cli.rs` and `Cargo.toml`,
  and expose the assets directory to tests through `cargo:rustc-env`.
- [x] Make generation deterministic and idempotent when the generated asset
  directories already contain files. Fail the build with a useful error if any
  requested artifact cannot be rendered or written.
- [x] Load settings in flag, environment, then default order. Support
  `--searxng-url`, `--log-level`, and `--no-color` with their documented
  environment variables.
- [x] Install one `tracing-subscriber` at process startup, filter it from the
  resolved log level, and write its formatted diagnostics only to stderr.
- [x] Create one Tokio cancellation token driven by SIGINT and SIGTERM and pass
  clones into command, HTTP, and MCP paths.
- [x] Reserve stdout for command results or MCP frames. Keep status, diagnostics,
  and errors on stderr.
- [x] Configure Clap to return concise usage errors for missing, extra, or
  unknown arguments without accepting abbreviated subcommands.

**Acceptance criteria:**

- [x] `xngmcp --help`, `xngmcp --version`, and `xngmcp serve --help` work.
- [x] `src/main.rs` delegates to `xngmcp::run()` and contains no application
  behavior beyond the async entry point.
- [x] A clean build produces a non-empty `xngmcp.1`, one page for every current
  subcommand, and a completion file for each supported shell; the generated
  artifacts contain the current commands and global options.
- [x] Changing a command or option in `src/cli.rs` updates runtime help, the man
  page, and completions without editing another command definition.
- [x] Flags override environment values, and invalid URLs or log levels fail
  before a command starts work.
- [x] Help and version output do not require SearXNG to be running.
- [x] Unit or integration tests cover configuration precedence, Clap exit
  behavior, stream selection, tracing output, cancellation propagation, and the
  presence and content of generated CLI artifacts.

**Verification:** `cargo test application_shell`,
`cargo test generated_cli_assets`, and `cargo run -- --help`

**Blocked by:** None - can start immediately.

## T02 — Add the SearXNG Compose stack

**What to build:** A self-contained local SearXNG and Valkey deployment under
`infra/searxng` that later feature tickets can test against.

**Work:**

- [x] Add `compose.yaml` with immutable image versions, loopback-only SearXNG
  binding, health checks, restart policy, and a disposable Valkey cache volume.
- [x] Add `settings.yml` with JSON search enabled, safe search level 1, bounded
  outgoing timeouts, Valkey caching, and private-instance settings.
- [x] Add `.env.example` and a portable `bootstrap.sh` that creates a mode-0600
  `.env` with a fresh secret and never overwrites an existing file.
- [x] Add repeatable smoke checks for `/healthz` and a JSON `/search` response.
- [x] Document start, stop, logs, health, image upgrades, configuration changes,
  and rollback in `infra/searxng/README.md`.

**Acceptance criteria:**

- [x] `docker compose config` succeeds without printing the generated secret.
- [x] A clean start and restart make both containers healthy.
- [x] The host port is configurable and bound to `127.0.0.1` by default.
- [x] Re-running bootstrap preserves the current secret and configuration.
- [x] A JSON search returns results suitable for adapter and CLI development.

**Verification:** Run Compose config, clean-start, restart, health, and JSON
search checks from the infra README.

**Blocked by:** None - can start immediately.

## T03 — Deliver normalized web search

**What to build:** A search service that turns a validated request into a small,
stable result set from SearXNG.

**Work:**

- [x] Define the search request, result, suggestion, and unavailable-engine
  types described in the roadmap.
- [x] Add `reqwest` and URL-handling dependencies at released versions and
  configure one reusable async SearXNG HTTP client.
- [x] Apply defaults and validate query length, result limit, page, time range,
  safe-search level, categories, engines, and domain lists in one shared entry
  point.
- [x] Encode valid requests for the SearXNG JSON API and preserve query text
  exactly.
- [x] Normalize upstream fields, reject invalid result URLs, deduplicate by
  normalized URL, and preserve upstream rank.
- [x] Apply include/exclude filters with exact-host or subdomain matching after
  results return.
- [x] Preserve useful results when SearXNG also reports unavailable engines.
- [x] Map timeouts, malformed responses, and backend failures to actionable
  application errors.
- [x] Add isolated HTTP fixture coverage with a local test server whose response
  bodies and status codes are controlled by the test.
- [x] Gate one ignored integration test behind an explicit SearXNG endpoint and
  run it against the T02 stack.

**Acceptance criteria:**

- [x] Empty or out-of-range inputs fail locally and use the roadmap's defaults
  when omitted.
- [x] A filtered result never escapes its domain rule through suffix matching,
  mixed case, or an explicit port.
- [x] Optional empty fields are omitted from JSON output.
- [x] Table-driven tests cover encoding, validation, deduplication, ordering,
  filtering, partial engine failures, and cancellation.
- [x] A real JSON search through the local stack normalizes successfully.

**Verification:** `cargo test search`, then run the ignored search integration
test against the T02 endpoint with its documented environment variable.

**Blocked by:** T01 and T02.

## T04 — Deliver hardened web fetch

**What to build:** A fetch service that returns readable Markdown or text from a
public HTTP(S) URL while preventing access to local networks.

**Work:**

- [x] Define the fetch request and result types, including final URL, title,
  content type, content, and truncation state.
- [x] Add `lectito` from its released crates.io package plus only the charset
  and HTTP connector dependencies needed by the implementation; do not use a
  local path or Git dependency.
- [x] Validate URL scheme, output format, and `max_chars`; apply documented
  defaults in the fetch service.
- [x] Resolve hosts, reject all blocked IPv4 and IPv6 ranges, and dial only the
  validated address.
- [x] Repeat host validation on redirects and enforce redirect, connect,
  response-header, operation, and decompressed-body limits.
- [x] Decode supported charsets and pass the HTML string and final URL to
  `lectito::extract`; select its Markdown or plain-text field, preserve its
  title and relevant metadata, and pass plain-text HTTP responses through.
- [x] Treat Lectito's no-readable-article result and extraction errors as
  actionable application errors; do not fall back to returning unfiltered HTML.
- [x] Truncate on Unicode boundaries and reject binary or unsupported media with
  an actionable error.
- [x] Ensure requests never forward credentials or cookies.

**Acceptance criteria:**

- [x] Direct, DNS-resolved, redirected, IPv4-mapped IPv6, and rebinding attempts
  cannot reach blocked addresses.
- [x] Oversized or slow responses stop at the configured limit and respect
  cancellation.
- [x] Article HTML, non-UTF-8 HTML, and plain text produce the documented result
  shape; navigation-only HTML returns an extraction error.
- [x] Adversarial tests cover every blocked address class and redirect hop.
- [x] Tests use fixed HTML fixtures to prove Lectito Markdown/text selection,
  title propagation, Unicode-safe truncation, and no-article handling.

**Verification:** `cargo test fetch`

**Blocked by:** T01.

## T05 — Expose search and fetch through the CLI

**What to build:** Human-readable terminal commands and stable script output for
both web features, following the command-line interface in the roadmap.

**Work:**

- [x] Add `search <query>` and `fetch <url>` as Clap subcommands with exact
  positional argument counts, value parsers, conflicts, and the documented
  feature flags.
- [x] Map repeated category, engine, and domain flags to the search request.
- [x] Serialize the feature result types directly for `--json`; do not create a
  second CLI-only schema.
- [x] Add compact `--plain` search output and restrained `owo-colors` styling
  for interactive terminals.
- [x] Disable styling for redirected output, `NO_COLOR`, `TERM=dumb`,
  `--no-color`, `--json`, and `--plain`.
- [x] Keep data on stdout and progress, diagnostics, and errors on stderr.
- [x] Document exit codes and add examples to command help; ensure no-argument,
  `-h`, `--help`, `help <command>`, `--version`, and typo behavior follow the
  CLI Guidelines decisions in the roadmap without network access.
- [x] Smoke-test `search` against the T02 stack, then fetch one returned URL.

**Acceptance criteria:**

- [x] CLI flags use the same defaults and validation errors as direct service
  calls.
- [x] JSON output is valid when redirected and is not mixed with logs.
- [x] Help is concise, includes common examples, and does not paginate or prompt.
- [x] Tests cover argument counts, repeated flags, output modes, color rules,
  stream separation, exit codes, and cancellation.
- [x] The real search-then-fetch smoke flow succeeds against local infra.

**Verification:** `cargo test cli`, then run the documented CLI smoke flow
against the T02 endpoint.

**Blocked by:** T03 and T04.

## T06 — Expose web search through MCP

**What to build:** A discoverable `web_search` MCP tool backed by the search
service.

**Work:**

- [x] Derive `serde::Deserialize` and `schemars::JsonSchema` for the shared
  search input and register `web_search` with rmcp's tool macros and an
  agent-oriented description.
- [x] Add released `rmcp` server and stdio transport features plus `schemars`,
  using the Tokio runtime already established by T01.
- [x] Map every tool field to the shared search request and return its result as
  structured content with a concise text fallback.
- [x] Return invalid inputs and expected SearXNG failures as tool errors rather
  than closing the MCP session.
- [x] Pass request cancellation through to the search operation.
- [x] Add an in-process rmcp client/server test for discovery, schema bounds,
  successful calls, validation failures, backend failures, and cancellation.
- [x] Compare a representative MCP structured result with `search --json` to
  catch interface drift inside this feature.

**Acceptance criteria:**

- [x] MCP discovery lists `web_search` with every documented field, bound, and
  default represented in its schema or description.
- [x] A successful call returns the same fields and omission behavior as CLI
  JSON.
- [x] A failed tool call leaves the MCP connection usable for the next call.

**Verification:** `cargo test web_search_mcp`

**Blocked by:** T03 and T05.

## T07 — Expose web fetch through MCP and harden stdio

**What to build:** A `web_fetch` MCP tool and a stdio server that remains valid
under logging, errors, cancellation, and shutdown.

**Work:**

- [x] Derive the rmcp input schema from the shared typed URL, `max_chars`, and
  format fields, and register `web_fetch` with an agent-oriented description.
- [x] Return the shared fetch result as structured content with a concise text
  fallback, including the final URL and truncation state.
- [x] Map validation, network policy, media, size, timeout, and extraction
  failures to actionable tool errors.
- [x] Add in-process rmcp discovery and call tests for success, failure, and
  cancellation.
- [x] Compare a representative MCP structured result with `fetch --json`.
- [x] Serve the handler with rmcp's Tokio stdio transport and add a subprocess
  test that enables verbose tracing and proves stdout still contains only valid
  MCP protocol messages.
- [x] Verify that EOF on stdin and process cancellation shut down promptly.

**Acceptance criteria:**

- [x] MCP discovery lists exactly `web_search` and `web_fetch`.
- [x] `web_fetch` enforces the same network and size rules as direct and CLI
  calls.
- [x] Logs and styled output never enter the protocol stream.
- [x] Tool errors do not terminate the stdio session.

**Verification:** `cargo test mcp` and `cargo test stdio`

**Blocked by:** T04, T05, and T06.

## T08 — Validate and cut over local infrastructure

**What to build:** A verified migration from any current local SearXNG service
to the stack owned by this repository.

**Work:**

- [x] Start the new stack on `127.0.0.1:18080` without changing the service on
  port 8080.
- [x] Run health, JSON search, CLI search and fetch, and both MCP tool smoke
  checks against the staged endpoint.
- [x] Record the current service's start and stop commands before cutover.
- [x] Stop the current service, move the validated stack to port 8080, and
  repeat all smoke checks after a restart.
- [x] Exercise rollback by stopping the new stack and restoring the prior
  service, then return to the new stack once the rollback check passes.

**Acceptance criteria:**

- [x] The repository-owned stack serves the target endpoint after restart.
- [x] Pi, Codex, OpenCode, and thndrs prerequisites can reach the endpoint from
  their process environments.
- [x] Rollback restores the prior service without relying on disposable Valkey
  data.

**Verification:** Follow the cutover and rollback procedure in the infra README.

**Blocked by:** T02 and T07.

## T09 — Integrate and verify Pi

**What to build:** An installable Pi package that makes xngmcp's two MCP tools
Pi's visible web capability on machines where Pi has no native web search.

**Work:**

- [x] Add `packages/pi-plugin-xngmcp` as a Pi package with a package manifest,
  an xngmcp skill, and a small extension that adds prompt guidance without
  registering replacement tools. The guidance tells Pi when current or
  external information requires web search, how to form focused queries, and
  when to fetch a chosen result.
- [x] Integrate through `pi-mcp-adapter`; do not add a TypeScript search client,
  fetch client, or second MCP transport implementation.
- [x] Document the adapter, binary `PATH`, healthy SearXNG endpoint, package
  installation, and project `.mcp.json` entry. Configure `directTools` for only
  `web_search` and `web_fetch` so both appear beside Pi's built-in tools.
- [x] Verify first-run behavior with an empty adapter metadata cache, then use
  `/mcp reconnect web` and Pi's normal reload flow to expose both direct tools.
- [x] Ask Pi for current information without mentioning MCP or xngmcp. Confirm
  it searches, chooses a returned public URL, and fetches it with a bounded
  character limit.
- [x] Use the adapter's `/mcp` panel for connection and tool diagnostics.
  Document the shortest checks for a missing package, missing binary, stale
  tool cache, launch failure, and unreachable SearXNG endpoint.

**Acceptance criteria:**

- [x] A fresh Pi installation can install the package and copy the example
  without machine-specific paths.
- [x] Pi lists `web_search` and `web_fetch` as direct tools after initial cache
  discovery and reload.
- [x] Pi selects search then fetch for a current-information request without the
  user naming MCP, xngmcp, or either tool.
- [x] The Pi package contains no search, fetch, MCP transport, or SearXNG client
  implementation.
- [x] Stopping SearXNG produces a visible tool error while Pi remains usable.

**Verification:** Install the local Pi package in a clean Pi agent directory,
run the documented empty-cache discovery and reconnect checks, then run the
implicit search-then-fetch request and backend-failure check.

**Blocked by:** T07 and T08.

## T10 — Integrate and verify Codex

**What to build:** Copyable project-scoped Codex setup instructions for the
same offline-agent web workflow.

**Work:**

- [ ] Add the `.codex/config.toml` example with the stdio command and SearXNG
  environment setting.
- [ ] Document project trust, binary `PATH`, and healthy SearXNG prerequisites.
- [ ] Run `codex mcp list` and inspect `/mcp` in the Codex TUI before attempting
  agent use.
- [ ] Ask Codex to perform the same bounded search-then-fetch flow used for Pi.
- [ ] Document diagnostics for trust, configuration, launch, and endpoint
  failures.

**Acceptance criteria:**

- [ ] A trusted project can copy the example without machine-specific paths.
- [ ] Codex lists the `web` server and exposes both tools.
- [ ] Codex completes search then fetch and surfaces backend failures as tool
  errors rather than ending the agent session.

**Verification:** Run the documented `codex mcp list`, `/mcp`, and agent flow.

**Blocked by:** T07 and T08.

## T11 — Integrate and verify OpenCode

**What to build:** Copyable OpenCode V2 setup instructions for the same
offline-agent web workflow.

**Work:**

- [ ] Add the project OpenCode configuration with a local server under
  `mcp.servers`, the `xngmcp serve` command array, and `SEARXNG_URL`
  environment substitution.
- [ ] Document binary `PATH`, environment, and healthy SearXNG prerequisites.
- [ ] Check the server's connection status through OpenCode's MCP management
  interface before attempting agent use.
- [ ] Ask OpenCode to perform the same bounded search-then-fetch flow used for
  Pi.
- [ ] Document diagnostics for configuration, launch, tool discovery, and
  endpoint failures.

**Acceptance criteria:**

- [ ] A fresh project can copy the example after setting `SEARXNG_URL`, without
  a machine-specific executable path.
- [ ] OpenCode reports the `web` server as connected and exposes both tools.
- [ ] OpenCode completes search then fetch and surfaces backend failures as tool
  errors rather than ending the agent session.

**Verification:** Run the documented connection-status and agent checks.

**Blocked by:** T07 and T08.

## T12 — Integrate and verify thndrs

**What to build:** Copyable thndrs setup instructions for the same offline-agent
web workflow.

**Work:**

- [ ] Add the `.thndrs/mcp.toml` example with stdio transport, environment, and
  timeout settings.
- [ ] Document project trust, binary `PATH`, and healthy SearXNG prerequisites.
- [ ] Run thndrs' MCP test and tool discovery before attempting agent use.
- [ ] Ask thndrs to perform the same bounded search-then-fetch flow used for Pi.
- [ ] Document diagnostics for trust, configuration, launch, and endpoint
  failures.

**Acceptance criteria:**

- [ ] A fresh project can copy the example without machine-specific paths.
- [ ] `mcp test web` passes and thndrs lists both tools.
- [ ] thndrs completes search then fetch and surfaces backend failures as tool
  errors rather than agent crashes.

**Verification:** Run the documented thndrs MCP test, discovery, and agent flow.

**Blocked by:** T07 and T08.

## T13 — Finish release documentation and checks

**What to build:** A release candidate that a user can install, start, connect,
and troubleshoot from the repository documentation.

**Work:**

- [ ] Keep the root README to the shortest install, infrastructure start, CLI
  smoke test, and agent integration path; link to detailed infra operations.
- [ ] Check every copied command and configuration example from a clean shell.
- [ ] Confirm command help, README examples, MCP schemas, and infra defaults use
  the same names and limits.
- [ ] Document where packagers find the build-generated man pages under
  `assets/man` and completion files under `assets/completions`, and include them
  in the release packaging smoke check.
- [ ] Run formatting, check, Clippy with warnings denied, tests, Compose
  validation, and the roadmap's Pi, Codex, OpenCode, and thndrs acceptance
  flows.
- [ ] Record the minimum supported Rust version, resolved crate versions from
  `Cargo.lock`, and container image versions for the release.

**Acceptance criteria:**

- [ ] A new user can follow the README without access to another local
  repository or machine-specific path.
- [ ] All automated checks pass and all four agent flows complete against the
  owned stack.
- [ ] Help and documentation describe recovery for expected configuration,
  backend, and network-policy failures.
- [ ] The release package installs or stages all generated man pages and all
  five completion files without regenerating a separate command definition.

**Verification:** Run `cargo fmt --all -- --check`,
`cargo check --all-targets --all-features`,
`cargo clippy --all-targets --all-features -- -D warnings`,
`cargo test --all-features`, Docker Compose validation, then the documented Pi,
Codex, OpenCode, and thndrs smoke tests.

**Blocked by:** T09, T10, T11, and T12.
