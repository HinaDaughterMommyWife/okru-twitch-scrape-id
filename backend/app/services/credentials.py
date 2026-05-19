"""
Twitch credentials store — persisted in data/twitch_credentials.json
"""

import json
from pathlib import Path

from app.core.config import settings


def credentials_exist() -> bool:
    return settings.CREDENTIALS_FILE.exists()


def load_credentials() -> dict | None:
    if not credentials_exist():
        return None
    with open(settings.CREDENTIALS_FILE) as f:
        return json.load(f)


def save_credentials(data: dict):
    settings.DATA_DIR.mkdir(parents=True, exist_ok=True)
    with open(settings.CREDENTIALS_FILE, "w") as f:
        json.dump(data, f, indent=2)


def clear_credentials():
    if settings.CREDENTIALS_FILE.exists():
        settings.CREDENTIALS_FILE.unlink()
