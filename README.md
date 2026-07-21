# okru-scraping

<div align="center">
<img src="resource/hina.webp" width="600" />
</div>

Detects active VK live streams and serves them through a web interface with an embedded Twitch chat.

Built for [thedarkraimola](https://www.twitch.tv/thedarkraimola).

## Components

- **`backend/`** — Rust binary: Twitch IRC bot (`twitch-irc`) + VK HTML scraper. Mods write `#vk` in chat → scrape `idvk` → POST `{vk_oid, vk_id}` to the worker. Periodic checks run for 8h after each `#vk`.
- **`worker/`** — Cloudflare Worker (Rust) that stores/serves the latest `vk_oid`/`vk_id` in KV (Basic Auth).
- **`web/`** — Astro SSR on Cloudflare Workers. Fetches IDs from the worker (or `?id=` override) and embeds the VK player + Twitch chat.

### Local development (pm2)

```bash
make install          # build → dist/okru-backend + config.toml if missing
$EDITOR dist/config.toml

make up               # pm2: backend + worker (:8787) + web (astro)
make logs
make down
```

Backend lee `dist/config.toml` (junto al binario). OAuth local: `{baseUrl}/{setupPathKey}/setup`.

### Oracle Linux 8 ARM build

Build on your machine, copy the binary to the server:

```bash
make build-oracle-arm
# → dist/okru-backend  (linux/arm64, glibc OL8)

scp dist/okru-backend dist/config.toml user@oracle-host:~/okru/
```

### Config (`config.toml` next to the binary)

```toml
idvk = "id1117440596"
intervalo = 60
twitchTokenId = "..."
twitchTokenSecret = "..."
channelTarget = "thedarkraimola"
botName = "comomegustapadreball"
postURL = "http://localhost:8787/streaming"
postAuth = "..."
setupPathKey = "..."   # auto-generated if empty
port = 9622
baseUrl = "http://localhost:9622"
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
cp .dev.vars.example .dev.vars  # WORKER_URL + WORKER_AUTH_TOKEN
pnpm dev
```

## Chat command

`#vk` — mods and broadcaster only. Safe-send: scrape + POST always run even if chat reply fails. Activates a periodic check loop for 8 hours (resets on each `#vk`).

## Manual web override

`/?id=video1117440596_456239034` (also `video-…` or bare `oid_vid`) bypasses the worker and embeds that stream directly.
