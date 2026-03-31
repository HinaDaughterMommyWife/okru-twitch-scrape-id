# okru-scraping

Automatically detects active ok.ru live streams and serves them through a simple web interface with an embedded Twitch chat.

Built for [thedarkraimola](https://www.twitch.tv/thedarkraimola).

## Components

- **`scripts/`** — Python scraper that monitors an ok.ru profile for active live streams. When one is found, it extracts the streaming ID and posts it to the worker API. Runs in Docker via Scrapling.
- **`worker/`** — Cloudflare Worker (Rust) that stores and serves the current streaming ID using KV. Protected with Basic Auth.
- **`web/`** — Astro site deployed on Cloudflare Workers. Fetches the streaming ID from the worker and renders the ok.ru video embed alongside a Twitch chat iframe.

## Setup

```bash
# Scraper
docker compose up

# Worker
cd worker
wrangler secret put AUTH_TOKEN
wrangler deploy

# Web
cd web
pnpm install
cp .dev.vars.example .dev.vars  # add your WORKER_AUTH_TOKEN
pnpm dev
```
