---
name: xngmcp
description: Use Pi's xngmcp web search and fetch tools for current or external information. Use when a task needs web research, source verification, or readable content from a public URL.
---

# xngmcp web research

Use `mcp__web__web_search` when the answer needs current or external
information. Form a specific query that names the subject and the fact to find.
Search before fetching rather than guessing a URL.

Read the result list, choose the most relevant public source, then call
`mcp__web__web_fetch` for that URL. Set `max_chars` to the smallest useful
limit; start with 12,000 characters and raise it only when the answer needs
more context.

Do not search for facts already supplied by the user or available in the local
project. Do not fetch a result merely because it ranked first.

## Setup

The package supplies guidance only. Install the xngmcp binary, start its local
SearXNG backend, and configure Pi's built-in MCP support at user scope. See the
[setup guide](../../README.md) for commands. Pi names the tools
`mcp__web__web_search` and `mcp__web__web_fetch` when the server is named
`web`.

Run `pi mcp list` to check that `web` is connected with two direct tools. In
an existing Pi session, use `/reload` after changing the configuration and
`/mcp` to inspect the connection.

## Troubleshooting

- Missing package: run `pi list`; install this package from the checkout.
- Missing binary: run `command -v xngmcp`; install it with
  `cargo install --path . --locked` from an xngmcp checkout.
- Missing server: run `pi mcp list`; add `web` at user scope as shown in the
  setup guide, then reload Pi.
- Unreachable SearXNG: check `http://127.0.0.1:8080/healthz`, start the
  Compose stack, and check `SEARXNG_URL` in `~/.pi/agent/mcp.json`.
