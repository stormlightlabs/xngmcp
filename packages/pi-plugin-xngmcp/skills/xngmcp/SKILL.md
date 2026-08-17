---
name: xngmcp
description: Use Pi's xngmcp web_search and web_fetch tools for current or external information. Use when a task needs web research, source verification, or readable content from a public URL.
---

# xngmcp web research

Use `web_search` when the answer needs current or external information. Form a
specific query that names the subject and the fact to find. Search before
fetching rather than guessing a URL.

Read the result list, choose the most relevant public source, then call
`web_fetch` for that URL. Set `max_chars` to the smallest useful limit; start
with 12,000 characters and raise it only when the answer needs more context.

Do not search for facts already supplied by the user or available in the local
project. Do not fetch a result merely because it ranked first.

## Setup

The package supplies guidance only. Install the MCP adapter, start xngmcp's
SearXNG backend, and configure the adapter to run the `xngmcp` binary.

```sh
pi install npm:pi-mcp-adapter
pi install npm:pi-plugin-xngmcp

xngmcp --version
curl --fail --silent --show-error http://127.0.0.1:8080/healthz >/dev/null
```

Create the project-root `.mcp.json` from the configuration in the package
README. The adapter finds this shared project configuration automatically.

Start Pi in that project. On the first run, refresh the empty metadata cache
and reload Pi's resources:

```text
/mcp reconnect web
/reload
```

`web_search` and `web_fetch` then appear as direct tools. Use `/mcp` to inspect
the connection and tool list.

## Troubleshooting

- Missing package: run `pi list`, then install `pi-mcp-adapter` and
  `pi-plugin-xngmcp`.
- Missing binary: run `command -v xngmcp`; install it with `cargo install --path .`
  from an xngmcp checkout, or put its install directory on `PATH`.
- Stale tool cache: run `/mcp reconnect web`, then `/reload`.
- Launch failure: open `/mcp`; temporarily set `"debug": true` for `web` in
  `.mcp.json`, reconnect, and read the server stderr shown by the panel.
- Unreachable SearXNG: run the health check above, correct `SEARXNG_URL` in
  `.mcp.json`, then reconnect. A backend failure is a tool error; Pi can
  continue with its other tools.
