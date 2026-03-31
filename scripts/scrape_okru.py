import re
import time
import traceback
import urllib.request
import json
import base64

from scrapling.fetchers import StealthyFetcher

URL = "https://ok.ru/live/profile/590655044274" # replace with the actual profile URL to monitor
CHECK_INTERVAL = 300  # 5 minutes in seconds
MAX_RETRIES = 5       # fetch attempts before giving up and waiting
RETRY_DELAY = 10      # seconds between retries
POST_URL = "https://httpbin.org/post"  # replace with: https://okru-worker.<subdomain>.workers.dev/streaming
AUTH_TOKEN = "changeme"  # must match the AUTH_TOKEN secret in the worker


def find_active_live_link(page):
    cards = page.css(".video-card.js-movie-card")
    print(f"  Found {len(cards)} video-card(s)")

    for i, card in enumerate(cards, 1):
        active_badge = card.css(".video-card_live.__active")
        if active_badge:
            anchor = card.css("a")
            if anchor:
                href = anchor[0].attrib.get("href", "")
                print(f"  Card [{i}] has active live badge -> href: {href}")
                return href
            else:
                print(f"  Card [{i}] has active live badge but no <a> found")
                return None

    print("  No card with an active live badge found.")
    return None


def extract_streaming_id(href):
    match = re.search(r"/live/(\d+)", href)
    if match:
        return match.group(1)
    return None


def send_post(streaming_id):
    credentials = base64.b64encode(f"admin:{AUTH_TOKEN}".encode()).decode()

    payload = json.dumps({
        "streaming_id": streaming_id,
        "source_url": URL,
        "timestamp": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
    }).encode("utf-8")

    req = urllib.request.Request(
        POST_URL,
        data=payload,
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Basic {credentials}",
            "User-Agent": "okru-scraper/1.0",
        },
        method="POST",
    )

    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            print(f"  POST {POST_URL} -> {resp.status}")
    except Exception as e:
        print(f"  POST failed: {e}")


def fetch_page_with_retries():
    for attempt in range(1, MAX_RETRIES + 1):
        try:
            print(f"  Attempt {attempt}/{MAX_RETRIES}...")
            page = StealthyFetcher.fetch(
                URL,
                headless=True,
                disable_resources=True,
                network_idle=False,
                timeout=60000,
                wait_selector=".video-card",
                wait_selector_state="attached",
            )
            return page
        except Exception as e:
            print(f"  Attempt {attempt}/{MAX_RETRIES} failed: {e}")
            if attempt < MAX_RETRIES:
                print(f"  Retrying in {RETRY_DELAY}s...")
                time.sleep(RETRY_DELAY)

    return None


def is_page_valid(page):
    status = page.status
    if status != 200:
        print(f"  Bad HTTP status: {status}")
        return False

    if not page.css("h1"):
        print("  Page has no <h1> -- likely blocked, captcha, or empty response")
        return False

    return True


def check_once():
    print(f"Fetching: {URL}")
    page = fetch_page_with_retries()

    if page is None:
        print(f"  All {MAX_RETRIES} attempts failed -- skipping POST to preserve current state")
        return None

    print(f"  Page fetched (status {page.status})")

    if not is_page_valid(page):
        print("  Page did NOT load correctly -- skipping POST to preserve current state")
        return None

    print("  Page loaded correctly")
    href = find_active_live_link(page)

    if href:
        streaming_id = extract_streaming_id(href)
        if streaming_id:
            print(f"  Active live stream: {streaming_id}")
            send_post(streaming_id)
        else:
            print(f"  href found but could not extract ID: {href}")
            send_post("NOT_FOUND")
    else:
        print("  No active live stream found -- sending NOT_FOUND")
        send_post("NOT_FOUND")

    return href


def main():
    print("=" * 60)
    print("ok.ru live stream checker")
    print(f"  Target : {URL}")
    print(f"  Interval: {CHECK_INTERVAL}s ({CHECK_INTERVAL // 60}m)")
    print(f"  POST to : {POST_URL}")
    print("=" * 60)

    while True:
        now = time.strftime("%Y-%m-%d %H:%M:%S")
        print(f"\n[{now}] Starting check...")
        print("-" * 60)

        try:
            check_once()
        except Exception:
            print("  FETCH ERROR -- skipping POST to preserve current state")
            traceback.print_exc()

        print("-" * 60)
        print(f"Next check in {CHECK_INTERVAL // 60} minutes...")
        time.sleep(CHECK_INTERVAL)


if __name__ == "__main__":
    main()
