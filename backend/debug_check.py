"""
Debug one-shot check.
Usage:
  docker compose run debug           # fast scraper
  docker compose run debug -- --slow # force scrapling
"""

import asyncio
import logging
import sys

logging.basicConfig(
    level=logging.DEBUG,
    format="%(asctime)s %(name)s %(levelname)s %(message)s",
    stream=sys.stdout,
)

sys.path.insert(0, "/app")


async def main():
    force_slow = "--slow" in sys.argv

    from app.core.config import settings
    from app.services.check_service import run_check

    print(f"[debug] force_slow={force_slow}")
    print(f"[debug] URL={settings.OKRU_PROFILE_URL}")
    print("-" * 60)

    found, sid = await run_check(force_slow=force_slow)

    print("-" * 60)
    print(f"Result: found={found} streaming_id={sid}")


if __name__ == "__main__":
    asyncio.run(main())
