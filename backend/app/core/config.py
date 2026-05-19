from pathlib import Path
from dotenv import load_dotenv
import os

# Load .env from backend dir
_env_path = Path(__file__).resolve().parent.parent.parent / ".env"
load_dotenv(_env_path)


class Settings:
    TWITCH_CLIENT_ID: str = os.environ["TWITCH_CLIENT_ID"]
    TWITCH_CLIENT_SECRET: str = os.environ["TWITCH_CLIENT_SECRET"]
    TWITCH_BOT_USERNAME: str = os.environ["TWITCH_BOT_USERNAME"]
    TWITCH_CHANNEL: str = os.environ["TWITCH_CHANNEL"]
    TWITCH_SETUP_PATH_KEY: str = os.environ["TWITCH_SETUP_PATH_KEY"]
    # Comma-separated usernames allowed to use #okru besides mods/broadcaster
    TWITCH_WHITELIST: set[str] = {
        u.strip().lower()
        for u in os.getenv("TWITCH_WHITELIST", "").split(",")
        if u.strip()
    }

    OKRU_PROFILE_URL: str = os.getenv(
        "OKRU_PROFILE_URL", "https://ok.ru/live/profile/590655044274"
    )

    POST_URL: str = os.environ["POST_URL"]
    POST_AUTH_TOKEN: str = os.environ["POST_AUTH_TOKEN"]

    PORT: int = int(os.getenv("PORT", "9622"))

    # Paths
    DATA_DIR: Path = Path(__file__).resolve().parent.parent.parent / "data"
    CREDENTIALS_FILE: Path = DATA_DIR / "twitch_credentials.json"


settings = Settings()
settings.DATA_DIR.mkdir(parents=True, exist_ok=True)
