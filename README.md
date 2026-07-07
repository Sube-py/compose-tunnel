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
- Vue 3 UI for servers, Compose discovery, tunnel start/stop, env profiles, logs, and settings.
- Env profiles in the desktop app can target a project directory, bind tunnel local ports to named variables, add extra env values, and write a managed block to that directory's `.env`.
- Env activation is scoped by target directory, so each project can have one active env while other projects keep their own active env.
- User config and state under the platform config directory via the `directories` crate.

## Run The CLI

```bash
cargo run -p compose-tunnel-cli -- init
cargo run -p compose-tunnel-cli -- server add staging --host staging.example.com --user deploy
cargo run -p compose-tunnel-cli -- server add staging-sudo --host staging.example.com --user deploy --docker-command "sudo -n docker"
cargo run -p compose-tunnel-cli -- server test staging
cargo run -p compose-tunnel-cli -- compose list --server staging
cargo run -p compose-tunnel-cli -- compose services --server staging --project myapp
cargo run -p compose-tunnel-cli -- open --server staging --project myapp --service db --target-port 5432
cargo run -p compose-tunnel-cli -- status
cargo run -p compose-tunnel-cli -- env db
cargo run -p compose-tunnel-cli -- env profile list
cargo run -p compose-tunnel-cli -- env profile save staging-db --target-dir ./myapp --tunnel-port db:staging_db:DATABASE_PORT --env DATABASE_HOST=127.0.0.1
cargo run -p compose-tunnel-cli -- env profile show staging-db
cargo run -p compose-tunnel-cli -- env profile use staging-db
cargo run -p compose-tunnel-cli -- env profile write staging-db
cargo run -p compose-tunnel-cli -- close db
```

## Run The Desktop App

```bash
pnpm install
pnpm tauri dev
```

## Env Profiles

The desktop Env page is list-first. Use **Add Env** to open a PrimeVue dialog, choose a target project directory, add tunnel port bindings, and add extra env values.

For a tunnel binding, the port variable name can be referenced by other env keys:

```env
staging-db=15432
DATABASE_PORT=${staging-db}
DATABASE_HOST=127.0.0.1
```

Click **Use Env** to make that profile active for its target directory. Only one env is active per target directory, but different project directories can activate different env profiles at the same time. **Write .env** writes or updates the compose-tunnel managed block in the selected directory's `.env`.

The CLI can create, update, inspect, and use the same env profiles with `compose-tunnel env profile save`, `list`, `show`, `use`, and `write`.

## Verify

```bash
cargo check --workspace
cargo test -p compose-tunnel-core
pnpm build
pnpm tauri build
```

## Notes

The MVP uses the system `ssh` binary so existing SSH config, keys, agents, and ProxyJump rules continue to work. Remote Docker access is performed by the per-server Docker command, which defaults to `docker` and can be set to `sudo -n docker` or another custom command.
