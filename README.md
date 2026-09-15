# okru-scraping

<div align="center">
<img src="resource/hina.webp" width="600" />
</div>

Detects active VK live streams and serves them through a web interface with an embedded Twitch chat — for many channels at once.

Built for [thedarkraimola](https://www.twitch.tv/thedarkraimola).

## Components

Rust workspace (`Cargo.toml` at the root) + Cloudflare apps:

- **`backend/`** — `okru-backend`: Twitch IRC bot (`twitch-irc`) + VK HTML scraper. Reads users from SQLite and **hot-reloads** them (joins/leaves channels, aliases, whitelist, idvk). Syncs each user's public profile to the worker; on a chat command it scrapes `idvk` → POST ids + VODs. Periodic checks run for 8h per user after each command.
- **`tui/`** — `okru-tui`: ratatui CRUD to add / edit / enable / delete users. Its `db` module (SQLite schema, validation, store) is also used by the backend (`default-features = false`, no ratatui).
- **`worker/`** — Cloudflare Worker (Rust) that stores profiles, live ids and VODs per slug in KV (Basic Auth).
- **`web/`** — Astro SSR on Cloudflare Workers: `/[user]` + `/[user]/vods`; `DEFAULT_USER` is served at `/` and `/vods`.

## Users (SQLite)

| field | notes |
|---|---|
| `slug` | URL: `watch.shonensemanal.site/<slug>` (a-z 0-9 _ -, reserved: `vods`, `api`, …) |
| `display_name` | page title |
| `twitch_channel` | channel the bot joins |
| `idvk` | VK profile/community to scrape |
| `commands` | aliases, all do the same: `#vk, #okru, !web` |
| `whitelist` | logins allowed besides mods/broadcaster: `user1, user2` |
| `enabled` | disabled = bot leaves the channel and the page is removed |

DB location (same for backend and TUI):

- `OKRU_DB` env var, if set
- **local** (everything): `./data/okru.db` — debug builds default to it, and `make up` (pm2), `make tui`, `make run` set `OKRU_DB` to it
- **server** (`arm/` release binaries): `okru.db` next to the binaries

Flow: the TUI writes SQLite and notifies the backend over a local TCP link (`127.0.0.1:ipcPort`, default `29622`) → the backend reloads immediately (`DB recargada`, `✓ JOIN #canal`) → `PUT`/`DELETE /users/:slug` on the worker → the page `/<slug>` exists (showing "no stream"). The backend pushes events back over the same link (reload, JOIN/PART, worker sync), shown in the TUI header (`● bot :29622`) and its "Actividad del bot" panel. If the backend is down, the change is applied when it starts (it always loads the DB on startup). On startup the backend also deletes worker profiles that no longer exist in the DB.

`ipcPort` and `webURL` live in `config.toml` and both read the same file: `OKRU_CONFIG`, or `config.toml` next to the binary. Locally everything (`make up`, `make dev-backend`, `./tui.sh`, `make tui`) uses `dist/config.toml`. A chat command in that channel scrapes VK and posts the stream + VODs.

## Local development

```bash
make dev-backend      # debug backend on ./data/okru.db (config.toml next to target/debug binary)
./tui.sh              # cargo run TUI on the same ./data/okru.db (also: make dev-tui)
make test             # cargo test --workspace
```

### Prod-like local stack (pm2)

```bash
make install          # release → dist/okru-backend + dist/okru-tui + config.toml if missing
$EDITOR dist/config.toml

make up               # pm2 (ecosystem.config.cjs): backend + worker (:8787) + web (:4321); restarts if running
make tui              # release TUI on ./data/okru.db (same DB as the pm2 backend)
make logs
make down
```

For local OAuth setup, use `{baseUrl}/{setupPathKey}/setup`.

### Oracle Linux 8 ARM build

Build on your machine, copy both binaries + config.toml to the server:

```bash
make build-oracle-arm   # → arm/okru-backend + arm/okru-tui
```

### Config (`config.toml` next to the binary)

```toml
intervalo = 60
twitchTokenId = "..."
twitchTokenSecret = "..."
botName = "comomegustapadreball"
workerURL = "http://localhost:8787"
postAuth = "..."
setupPathKey = "..."   # auto-generated if empty
port = 9622
baseUrl = "http://localhost:9622"
ipcPort = 29622        # local TCP link with okru-tui (127.0.0.1 only)
webURL = "http://localhost:4321"   # web base shown in the TUI (prod: https://watch.shonensemanal.site)
```

### Optional tunnel (expose `/setup` OAuth)

```bash
make tunnel       # cloudflared → localhost:9622
make tunnel-down
```

## Worker / Web

```bash
cd worker
wrangler secret put AUTH_TOKEN
wrangler deploy

cd web
pnpm install
cp .dev.vars.example .dev.vars  # WORKER_URL + WORKER_AUTH_TOKEN (+ DEFAULT_USER, also in wrangler.jsonc vars)
pnpm dev
```

Worker API (Basic Auth `admin:<AUTH_TOKEN>`, KV keys `user:`, `stream:`, `vods:` per slug, no cache TTL):

| method | path | |
|---|---|---|
| GET | `/users` | `{ slugs: [...] }` |
| GET | `/users/:slug` | `{ user, streaming }` |
| PUT / DELETE | `/users/:slug` | upsert profile / remove everything |
| POST / GET | `/users/:slug/streaming` | `{ vk_oid, vk_id, user? }` |
| POST / GET | `/users/:slug/vods` | `{ items: [...] }` |

Web pages send `Cache-Control: private, no-store`; `/api/lastStream?user=<slug>` polls live ids.

## Chat commands

Any alias configured for the channel — mods, broadcaster and whitelisted users. Debounced per channel (40s). Safe-send: scrape + POST always run even if the chat reply fails. Activates a periodic check loop for that user for 8 hours (resets on each command).

## Manual web override

`/?id=video1117440596_456239034` or `/<slug>?id=…` (also `video-…` or bare `oid_vid`) embeds that stream directly.
