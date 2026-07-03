# Release and Deploy

`Release and deploy` builds the cloud frontend, `cloud-api`, and `local-agent` on GitHub Actions, then uploads a release tarball plus checksums. The cloud UI can use this GitHub Release as the source for the in-app “New update available” button.

The workflow can also deploy directly over SSH, but that is optional. The product update path is:

1. Push a `v*` tag.
2. GitHub Actions publishes `rust-watcher-cloud-linux-x86_64-<tag>.tar.gz`.
3. A running cloud server sees the newer release.
4. The user clicks `Update` in the frontend.
5. The server downloads the release asset, replaces binaries/frontend, writes `VERSION`, and restarts its user systemd service.

Required repository secrets:

- `RUST_WATCHER_DEPLOY_HOST`: server host, for example `212.192.223.204`
- `RUST_WATCHER_DEPLOY_USER`: SSH user, for example `resai`
- `RUST_WATCHER_DEPLOY_SSH_KEY`: private SSH key that can log in as the deploy user

Optional repository secrets/variables:

- secret `RUST_WATCHER_DEPLOY_PORT`: SSH port, defaults to `22`
- variable `RUST_WATCHER_DEPLOY_PATH`: app path, defaults to `/home/resai/apps/rust_watcher`
- variable `RUST_WATCHER_DEPLOY_SERVICE`: user systemd service, defaults to `rust-watcher-cloud-api.service`

Release flow:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Both the workflow deploy step and the in-app updater keep the existing `.env`, database, blobs, and workspaces. They only replace:

- `target/release/cloud-api`
- `target/release/local-agent`
- `frontend/dist`
- `VERSION`
