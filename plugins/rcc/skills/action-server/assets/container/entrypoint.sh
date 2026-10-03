#!/bin/sh
set -eu

api_key_file=${ACTION_SERVER_API_KEY_FILE:-/run/secrets/action-server-api-key}
if [ ! -r "$api_key_file" ]; then
    echo "Action Server API-key file is not readable: $api_key_file" >&2
    exit 1
fi
api_key=$(cat "$api_key_file")
if [ -z "$api_key" ] || [ "$api_key" = None ]; then
    echo "Action Server API-key file must contain a nonempty authentication key." >&2
    exit 1
fi

# Native startup synchronization imports the package and retires removed actions.
test -w /var/lib/action-server
exec action-server start \
    --address 0.0.0.0 --port 8080 \
    --dir /opt/actions --datadir /var/lib/action-server --actions-sync=true \
    --api-key "$api_key" --min-processes 1 --max-processes 2 "$@"
