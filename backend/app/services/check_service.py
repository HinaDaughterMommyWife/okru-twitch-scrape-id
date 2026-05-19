"""
Orchestrator: fast_scraper (3x) → scrapling (2x) → fail.
Notifies POST_URL on stream found.
"""

import asyncio
import base64
import json
import logging
import time

import httpx

from app.core.config import settings
from app.services.fast_scraper import scrape_fast_with_retries
from app.services.scrapling_scraper import scrape_scrapling_with_retries

logger = logging.getLogger("okru.check")


async def _post_streaming_id(streaming_id: str):
    payload = {
        "streaming_id": streaming_id,
        "source_url": settings.OKRU_PROFILE_URL,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }
    credentials = base64.b64encode(
        f"admin:{settings.POST_AUTH_TOKEN}".encode()
    ).decode()
    headers = {
        "Content-Type": "application/json",
        "Authorization": f"Basic {credentials}",
        "User-Agent": "okru-scraper/1.0",
    }
    try:
        async with httpx.AsyncClient(timeout=10) as client:
            resp = await client.post(settings.POST_URL, json=payload, headers=headers)
            logger.info("POST %s -> %s", settings.POST_URL, resp.status_code)
            return resp.status_code
    except Exception as e:
        logger.warning("POST failed: %s", e)
        return str(e)


def _run_fast(url: str):
    return scrape_fast_with_retries(url, retries=3, gap=2.0)


def _run_scrapling(url: str):
    return scrape_scrapling_with_retries(url, retries=2, gap=2.0)


async def run_check(force_slow: bool = False) -> tuple[bool, str | None]:
    """
    Run full check pipeline.
    Returns (found, streaming_id).
    """
    url = settings.OKRU_PROFILE_URL
    loop = asyncio.get_event_loop()

    found, sid = False, None

    if not force_slow:
        logger.info("Trying fast scraper...")
        try:
            found, sid = await loop.run_in_executor(None, _run_fast, url)
            logger.info("Fast scraper done: found=%s sid=%s", found, sid)
        except RuntimeError as e:
            logger.warning("Fast scraper failed: %s — falling back to scrapling", e)
            try:
                found, sid = await loop.run_in_executor(None, _run_scrapling, url)
                logger.info("Scrapling done: found=%s sid=%s", found, sid)
            except RuntimeError as e2:
                logger.error("Scrapling also failed: %s", e2)
                return False, None
    else:
        logger.info("Forced slow mode — using scrapling directly...")
        try:
            found, sid = await loop.run_in_executor(None, _run_scrapling, url)
            logger.info("Scrapling done: found=%s sid=%s", found, sid)
        except RuntimeError as e:
            logger.error("Scrapling failed: %s", e)
            return False, None

    if found and sid:
        await _post_streaming_id(sid)
    else:
        await _post_streaming_id("NOT_FOUND")

    return found, sid
