# xngmcp for Pi

This package helps Pi use xngmcp's web search and page fetch tools. Pi 1.x
connects to the MCP server directly; you don't need an MCP adapter.

## Install for every Pi project

Install the binary and start the local SearXNG backend from the repository root:

```sh
cargo install --path . --locked
./infra/searxng/bootstrap.sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

Add the server to your personal Pi configuration. Use the absolute path to the
installed binary so Pi can start it even when its `PATH` differs from your shell:

```sh
pi mcp add web --exposure direct \
  --description 'Search the public web and fetch readable pages using local xngmcp' \
  --env SEARXNG_URL=http://127.0.0.1:8080 \
  -- "$(command -v xngmcp)" serve
pi install ./packages/pi-plugin-xngmcp
```

Run these commands without `--local`: they configure `~/.pi/agent/mcp.json` and
`~/.pi/agent/settings.json` for all your Pi projects. The package is loaded
from this checkout, so keep it in place. Restart Pi or run `/reload` in an
existing session after installing it.

Pi exposes the tools as `mcp__web__web_search` and `mcp__web__web_fetch`.
The package adds brief guidance when both tools are active. You can also use
`/skill:xngmcp` for detailed instructions.

## Check the setup

```sh
curl --fail --silent --show-error http://127.0.0.1:8080/healthz
xngmcp search --limit 2 'Rust programming language'
pi mcp list
pi list
```

`pi mcp list` must show `web` connected with two direct tools. In an open Pi
session, run `/mcp` to inspect the connection. If the backend is down, start
Docker and rerun the Compose command above; Pi remains usable, but searches
will fail until SearXNG is available.

If you use a different SearXNG port, update `SEARXNG_URL` in the `pi mcp add`
command and in the health check. See the [SearXNG guide](../../infra/searxng/README.md)
for operations and troubleshooting.
