# Local SearXNG

This Compose stack runs SearXNG on the loopback interface with an internal
Valkey service. Valkey has no host port, and its named volume contains only
disposable search cache data. SearXNG enables JSON responses, safe search level
1, and five-second outgoing request timeouts.

The loopback binding prevents other machines from connecting to SearXNG through
the host port. Upstream search engines and the network between this machine and
them can still observe queries.

## Start and verify

Run these commands from the repository root. The bootstrap script creates an
ignored `.env` with mode 0600. Running it again preserves that file.

```sh
./infra/searxng/bootstrap.sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml config --quiet
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait

curl --fail --silent --show-error http://127.0.0.1:8080/healthz >/dev/null
curl --fail --silent --show-error --get http://127.0.0.1:8080/search \
  --data-urlencode 'q=rust programming language' \
  --data-urlencode 'format=json' \
  | jq --exit-status '.results | type == "array" and length > 0' >/dev/null
```

Set `SEARXNG_PORT` in `infra/searxng/.env` to change the host port. Compose
always binds it to `127.0.0.1`; the default is 8080.

Check container health and follow logs:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml ps
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml logs --follow
```

Restart both services and repeat the smoke checks:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml restart
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

Repeat the health and JSON search commands under "Start and verify."

## Run the search integration test

After the JSON smoke check succeeds, point the ignored search test at this
stack:

```sh
XNGMCP_TEST_SEARXNG_URL=http://127.0.0.1:8080 \
  cargo test search_integration -- --ignored
```

Set the URL to the configured loopback port when `SEARXNG_PORT` differs from
8080.

## Stop or remove the stack

Stop and remove the containers while retaining the Valkey cache volume:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml down
```

Remove the containers, orphaned services, and disposable cache volume:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml down --volumes --remove-orphans
```

Neither command removes `infra/searxng/.env`.

## Change configuration or images

After editing `settings.yml`, recreate SearXNG and run the smoke checks:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait --force-recreate searxng
```

Then repeat the health and JSON search commands under "Start and verify."

Image references include both an immutable version tag and digest. To upgrade,
replace one reference in `compose.yaml`, pull it, recreate the stack, and run
the smoke checks:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml pull
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

Then repeat the health and JSON search commands under "Start and verify."

To roll back, restore the previous image reference or settings, then run the
same `up -d --wait` and smoke-check commands. Remove the Valkey volume if a
configuration change makes its disposable cache incompatible:

```sh
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml down --volumes
docker compose --env-file infra/searxng/.env \
  -f infra/searxng/compose.yaml up -d --wait
```

Then repeat the health and JSON search commands under "Start and verify."
