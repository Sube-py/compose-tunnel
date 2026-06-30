# compose-tunnel

`compose-tunnel` forwards internal Docker Compose services on a remote server to a local port through SSH. It starts a temporary `socat` container inside the target Compose network and forwards local traffic directly to that container IP, without publishing a remote host port.

```text
127.0.0.1:localPort
  -> SSH LocalForward
  -> remote socat container IP:socatPort
  -> compose service name:targetPort
```

## What Is Implemented

- Shared Rust core crate for config, state, SSH, Docker, tunnel lifecycle, cleanup, and env file blocks.
- CLI binary named `compose-tunnel`.
- Tauri 2 desktop app using the same Rust core.
- Vue 3 UI for servers, Compose discovery, tunnel start/stop, env preview/write, logs, and settings.
- User config and state under the platform config directory via the `directories` crate.

## Run The CLI

```bash
cargo run -p compose-tunnel-cli -- init
cargo run -p compose-tunnel-cli -- server add staging --host staging.example.com --user deploy
cargo run -p compose-tunnel-cli -- server test staging
cargo run -p compose-tunnel-cli -- compose list --server staging
cargo run -p compose-tunnel-cli -- compose services --server staging --project myapp
cargo run -p compose-tunnel-cli -- open --server staging --project myapp --service db --target-port 5432
cargo run -p compose-tunnel-cli -- status
cargo run -p compose-tunnel-cli -- env db
cargo run -p compose-tunnel-cli -- close db
```

## Run The Desktop App

```bash
npm install
npm run tauri dev
```

## Verify

```bash
cargo check --workspace
cargo test -p compose-tunnel-core
npm run build
```

## Notes

The MVP uses the system `ssh` binary so existing SSH config, keys, agents, and ProxyJump rules continue to work. Remote Docker access is performed by running `docker` over SSH.
