# Cloud API

## Cloud API auth

`cloud-api` refuses to start with the default development credentials unless you explicitly opt in to insecure local auth. The blocked defaults are:

- `RUST_WATCHER_ADMIN_USERNAME=admin`
- `RUST_WATCHER_ADMIN_PASSWORD=dev-password`
- `RUST_WATCHER_DEV_TOKEN=dev-token`

For local development, opt in explicitly:

```bash
RUST_WATCHER_ALLOW_INSECURE_DEV_AUTH=true cargo run -p cloud-api -- serve
```

Do not set `RUST_WATCHER_ALLOW_INSECURE_DEV_AUTH=true` on a public server.

Production deployments must set non-default auth values:

```bash
export RUST_WATCHER_ADMIN_USERNAME=admin
export RUST_WATCHER_ADMIN_PASSWORD='replace-with-a-long-random-password'
export RUST_WATCHER_DEV_TOKEN='replace-with-a-long-random-agent-token'
export RUST_WATCHER_INTERNAL_API_TOKEN='replace-with-a-long-random-internal-token'
export RUST_WATCHER_AUTH_SESSION_TTL_SECONDS=86400

cargo run -p cloud-api -- serve
```

Auth variables:

- `RUST_WATCHER_ADMIN_USERNAME`: default cloud login username. Must not be the insecure default pair in production.
- `RUST_WATCHER_ADMIN_PASSWORD`: default cloud login password. Use a long random value.
- `RUST_WATCHER_USERS`: optional comma-separated `username:password` list. If set, these users replace the single admin username/password pair.
- `RUST_WATCHER_DEV_TOKEN`: token used by trusted local-agent flows. It is not accepted as a browser cloud session token.
- `RUST_WATCHER_INTERNAL_API_TOKEN`: required for legacy internal endpoints such as `/api/workspaces`, `/api/analysis/jobs`, and `/api/usage/summary`. Send it as `Authorization: Bearer <token>` or `X-Rust-Watcher-Token: <token>`.
- `RUST_WATCHER_AUTH_SESSION_TTL_SECONDS`: cloud browser session TTL. The default is `86400` seconds.
- `RUST_WATCHER_ALLOW_INSECURE_DEV_AUTH`: set to `true` only for local development when using default credentials.

Cloud browser endpoints under `/api/cloud/...` use login sessions returned by `POST /api/cloud/auth/login`. Legacy internal endpoints are separate and require `RUST_WATCHER_INTERNAL_API_TOKEN`.

The cloud websocket endpoint is authenticated too. Browser clients must connect with a valid cloud session token:

```text
GET /api/cloud/ws?token=<session_token>
```

Missing, invalid, or expired websocket tokens are rejected before upgrade.

## Systemd example

Example user service for a second cloud instance:

```ini
[Unit]
Description=Rust Watcher Cloud API second instance
After=network.target

[Service]
WorkingDirectory=/home/resai/apps/rust_watcher_second
Environment=RUST_LOG=cloud_api=info,tower_http=info
Environment=RUST_WATCHER_ADMIN_USERNAME=admin2
Environment=RUST_WATCHER_ADMIN_PASSWORD=replace-with-a-long-random-password
Environment=RUST_WATCHER_DEV_TOKEN=replace-with-a-long-random-agent-token
Environment=RUST_WATCHER_INTERNAL_API_TOKEN=replace-with-a-long-random-internal-token
Environment=RUST_WATCHER_AUTH_SESSION_TTL_SECONDS=86400
ExecStart=/home/resai/apps/rust_watcher_second/target/release/cloud-api serve \
  --host 127.0.0.1 \
  --port 34128 \
  --frontend-dist /home/resai/apps/rust_watcher_second/frontend/dist \
  --blobs-dir /home/resai/.local/share/rust-watcher-cloud-second/blobs \
  --workspaces-dir /home/resai/.local/share/rust-watcher-cloud-second/workspaces \
  --db-path /home/resai/.local/share/rust-watcher-cloud-second/cloud-api.sqlite
Restart=on-failure
RestartSec=3

[Install]
WantedBy=default.target
```

Use distinct ports, storage directories, credentials, and reverse-proxy hostnames for each instance.
