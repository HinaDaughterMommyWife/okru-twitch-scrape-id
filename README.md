# okru-scraping

<div align="center">
<img src="resource/hina.webp" width="600" />
</div>

Automatically detects active ok.ru live streams and serves them through a simple web interface with an embedded Twitch chat.

Built for [thedarkraimola](https://www.twitch.tv/thedarkraimola).

## Components

- **`backend/`** — FastAPI server that runs a Twitch bot (twitchio v3 EventSub). When a mod writes `#okru` in chat, it scrapes the ok.ru profile (fast stdlib scraper → Scrapling headless fallback) and posts the result to the worker.
- **`worker/`** — Cloudflare Worker (Rust) that stores and serves the latest streaming ID and timestamp using KV. Protected with Basic Auth.
- **`web/`** — Astro SSR site on Cloudflare Workers. Fetches the streaming ID from the worker and renders the ok.ru embed with a Twitch chat iframe.

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
