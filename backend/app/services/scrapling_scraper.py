"""
Scrapling-based scraper — headless browser fallback.
Used when fast_scraper fails all retries.
Up to 2 attempts.
"""

import re
import logging
import time

logger = logging.getLogger("okru.scrapling_scraper")


def _extract_streaming_id(href: str) -> str | None:
    m = re.search(r"/live/(\d+)", href)
    return m.group(1) if m else None


def _scrape_once(url: str) -> tuple[bool, str | None]:
    from scrapling.fetchers import StealthyFetcher  # lazy import

    page = StealthyFetcher.fetch(
        url,
        headless=True,
        disable_resources=True,
        network_idle=False,
        timeout=60000,
        wait_selector=".video-card",
        wait_selector_state="attached",
    )

    if page.status != 200:
        raise RuntimeError(f"bad HTTP status {page.status}")
    if not page.css("h1"):
        raise RuntimeError("page has no <h1> — likely blocked")

    cards = page.css(".video-card.js-movie-card")
    logger.debug("Scrapling found %d cards", len(cards))
    for card in cards:
        badge = card.css(".video-card_live.__active")
        anchor = card.css("a")
        href = anchor[0].attrib.get("href", "") if anchor else None
        logger.debug("  card: href=%s active_live=%s", href, bool(badge))
        if badge:
            if anchor and href:
                return True, _extract_streaming_id(href)
    return False, None


def scrape_scrapling_with_retries(url: str, retries: int = 2, gap: float = 2.0):
    last_exc = None
    for attempt in range(1, retries + 1):
        try:
            return _scrape_once(url)
        except Exception as e:
            last_exc = e
            if attempt < retries:
                time.sleep(gap)
    raise RuntimeError(f"scrapling failed {retries} times") from last_exc
