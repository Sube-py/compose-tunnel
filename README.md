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
cargo run -p compose-tunnel-cli -- server delete staging --yes
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
cargo run -p compose-tunnel-cli -- env profile write staging-db --yes
cargo run -p compose-tunnel-cli -- env profile delete staging-db
cargo run -p compose-tunnel-cli -- close db
cargo run -p compose-tunnel-cli -- close --all --yes
cargo run -p compose-tunnel-cli -- cleanup --server staging --dry-run
cargo run -p compose-tunnel-cli -- cleanup --server staging --yes
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
staging_db=15432
DATABASE_PORT=${staging_db}
DATABASE_HOST=127.0.0.1
```

Click **Use Env** to make that profile active for its target directory. Only one env is active per target directory, but different project directories can activate different env profiles at the same time. **Write .env** writes or updates the single compose-tunnel env profile block in the selected directory's `.env`, replacing the previously written active env for that project.

The CLI can create, update, inspect, use, write, and delete the same env profiles with `compose-tunnel env profile save`, `list`, `show`, `use`, `write`, and `delete`.

The CLI and desktop app ask for confirmation before writing extra env keys that look sensitive, such as `PASSWORD`, `TOKEN`, `SECRET`, or `PRIVATE_KEY`. Use `--yes` with `compose-tunnel env profile write` for non-interactive scripts.

## Cleanup

`compose-tunnel cleanup --server <name> --dry-run` lists managed `compose-tunnel` containers that are safe to remove. Without `--dry-run`, the CLI shows the same list and asks for confirmation before deleting remote containers. Use `--yes` for non-interactive scripts. The desktop app previews the cleanup list and asks for confirmation before deleting remote containers.

## Verify

```bash
cargo check --workspace
cargo test -p compose-tunnel-core
pnpm build
pnpm tauri build
```

## Notes

The MVP uses the system `ssh` binary so existing SSH config, keys, agents, and ProxyJump rules continue to work. Remote Docker access is performed by the per-server Docker command, which defaults to `docker` and can be set to `sudo -n docker` or another custom command.
