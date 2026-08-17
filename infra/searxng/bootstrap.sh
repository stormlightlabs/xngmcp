#!/bin/sh
set -eu

script_directory=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
environment_file="$script_directory/.env"

if [ -e "$environment_file" ]; then
	printf '%s\n' "Keeping existing $environment_file"
	exit 0
fi

if ! command -v openssl >/dev/null 2>&1; then
	printf '%s\n' 'openssl is required to generate the local SearXNG secret.' >&2
	exit 1
fi

umask 077
secret=$(openssl rand -hex 32)
printf 'SEARXNG_SECRET=%s\n' "$secret" >"$environment_file"
printf '%s\n' "Created $environment_file with mode 600"

