"""
Fast scraper — stdlib only, no headless browser.
Primary strategy: 3 attempts, 2s gap.
Returns (found: bool, streaming_id: str | None).
"""

import gzip
import http.cookiejar
import logging
import re
import time
import urllib.error
import urllib.request
from html.parser import HTMLParser

logger = logging.getLogger("okru.fast_scraper")

_BROWSER_HEADERS = [
    ("User-Agent", "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36"),
    ("sec-ch-ua", '"Chromium";v="124", "Google Chrome";v="124", "Not-A.Brand";v="99"'),
    ("sec-ch-ua-mobile", "?0"),
    ("sec-ch-ua-platform", '"Linux"'),
    ("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,image/avif,image/webp,*/*;q=0.8"),
    ("Sec-Fetch-Site", "none"),
    ("Sec-Fetch-Mode", "navigate"),
    ("Sec-Fetch-User", "?1"),
    ("Sec-Fetch-Dest", "document"),
    ("Accept-Encoding", "gzip, deflate"),
    ("Accept-Language", "en-US,en;q=0.9"),
]

_cookie_jar = http.cookiejar.CookieJar()
_opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(_cookie_jar))


def _decode_body(raw: bytes, encoding: str | None) -> str:
    if encoding in ("gzip", ""):
        try:
            return gzip.decompress(raw).decode("utf-8", errors="replace")
        except Exception:
            pass
    if encoding == "deflate":
        try:
            import zlib
            return zlib.decompress(raw).decode("utf-8", errors="replace")
        except Exception:
            pass
    # fallback: try gzip anyway
    try:
        return gzip.decompress(raw).decode("utf-8", errors="replace")
    except Exception:
        pass
    return raw.decode("utf-8", errors="replace")


def _fetch(url: str, referer: str | None = None) -> str | None:
    headers = list(_BROWSER_HEADERS)
    if referer:
        headers.append(("Referer", referer))
    req = urllib.request.Request(url)
    for k, v in headers:
        req.add_unredirected_header(k, v)
    try:
        with _opener.open(req, timeout=15) as resp:
            raw = resp.read()
            enc = resp.headers.get("Content-Encoding", "")
            return _decode_body(raw, enc)
    except (urllib.error.HTTPError, urllib.error.URLError):
        return None


class _VideoCardParser(HTMLParser):
    def __init__(self):
        super().__init__()
        self.active_live_href: str | None = None
        self._in_card = False
        self._depth = 0
        self._active_in_current_card = False
        self._card_href: str | None = None
        self.all_cards: list[dict] = []  # debug: all cards seen

    @staticmethod
    def _classes(attrs):
        for name, value in attrs:
            if name == "class":
                return set(value.split())
        return set()

    def handle_starttag(self, tag, attrs):
        if self.active_live_href:
            return
        classes = self._classes(attrs)
        if {"video-card", "js-movie-card"}.issubset(classes):
            self._in_card = True
            self._depth = 1
            self._active_in_current_card = False
            self._card_href = None
            return
        if self._in_card:
            self._depth += 1
            if tag == "a":
                for name, val in attrs:
                    if name == "href":
                        self._card_href = val
            if {"video-card_live", "__active"}.issubset(classes):
                self._active_in_current_card = True

    def handle_endtag(self, tag):
        if self._in_card:
            self._depth -= 1
            if self._depth <= 0:
                    if self._active_in_current_card and self._card_href:
                        self.active_live_href = self._card_href
                    self.all_cards.append({
                        "href": self._card_href,
                        "active_live": self._active_in_current_card,
                    })
                    self._in_card = False
                    self._depth = 0


def _extract_streaming_id(href: str) -> str | None:
    m = re.search(r"/live/(\d+)", href)
    return m.group(1) if m else None


def scrape_fast(url: str) -> tuple[bool, str | None]:
    """
    Returns (stream_found, streaming_id).
    streaming_id is None when not found.
    """
    # Warm cookies
    _fetch("https://ok.ru/")
    html = _fetch(url, referer="https://www.google.com/")
    if not html:
        return False, None

    parser = _VideoCardParser()
    parser.feed(html)

    logger.debug("Fast scraper found %d cards: %s", len(parser.all_cards), parser.all_cards)

    if parser.active_live_href:
        sid = _extract_streaming_id(parser.active_live_href)
        return True, sid
    return False, None


def scrape_fast_with_retries(url: str, retries: int = 3, gap: float = 2.0):
    """
    Try scrape_fast up to `retries` times with `gap` seconds between.
    Returns (found, streaming_id) or raises RuntimeError on all failures.
    """
    last_exc = None
    for attempt in range(1, retries + 1):
        try:
            result = scrape_fast(url)
            return result
        except Exception as e:
            last_exc = e
            if attempt < retries:
                time.sleep(gap)
    raise RuntimeError(f"fast scraper failed {retries} times") from last_exc
