"""
Twitch bot via twitchio v2 (IRC).
- Listens for chat messages via IRC (no mod requirement to send)
- Handles #okru from mods/broadcaster/whitelist
- Undebounce: first call fires, subsequent ignored until done or 40s
- Handles: emote-only, slow mode, chat-disabled silently
"""

import asyncio
import logging
import time

import twitchio
from twitchio.ext import commands

from app.core.config import settings
from app.services.credentials import load_credentials

logger = logging.getLogger("okru.bot")

_SLOW_MODE_DEFAULT = 5.0
_DEBOUNCE_WINDOW = 40.0

_bot_instance: "OkruBot | None" = None


class OkruBot(commands.Bot):
    def __init__(self, token: str):
        super().__init__(
            token=token,
            client_id=settings.TWITCH_CLIENT_ID,
            nick=settings.TWITCH_BOT_USERNAME,
            prefix="!",
            initial_channels=[settings.TWITCH_CHANNEL],
        )
        self._checking = False
        self._check_start: float = 0.0
        self._slow_mode: float = _SLOW_MODE_DEFAULT
        self._emote_only: bool = False
        self._chat_disabled: bool = False
        self._last_msg_time: float = 0.0

    # ------------------------------------------------------------------
    # Connected
    # ------------------------------------------------------------------
    async def event_ready(self):
        logger.info(
            "Bot ready — nick=%s, channel=%s",
            self.nick,
            settings.TWITCH_CHANNEL,
        )

    # ------------------------------------------------------------------
    # Safe send
    # ------------------------------------------------------------------
    async def _safe_send(self, channel: twitchio.Channel, text: str, emote_text: str | None = None):
        if self._chat_disabled:
            logger.info("Chat disabled — silent, skipping: %s", text)
            return

        msg = emote_text if (self._emote_only and emote_text) else text

        now = time.monotonic()
        since_last = now - self._last_msg_time
        if since_last < self._slow_mode:
            wait = self._slow_mode - since_last
            logger.info("Slow mode — waiting %.1fs", wait)
            await asyncio.sleep(wait)

        try:
            await channel.send(msg)
            self._last_msg_time = time.monotonic()
            logger.info("Sent: %s", msg)
        except Exception as e:
            logger.warning("Send failed (non-fatal): %s", e)

    # ------------------------------------------------------------------
    # Message event
    # ------------------------------------------------------------------
    async def event_message(self, message: twitchio.Message):
        # Ignore echoes from the bot itself
        if message.echo:
            return

        text = (message.content or "").strip().lower()

        if text != "#okru":
            return

        author = message.author
        if not author:
            return

        is_mod = getattr(author, "is_mod", False) or False
        is_broadcaster = getattr(author, "is_broadcaster", False) or False
        is_whitelisted = (author.name or "").lower() in settings.TWITCH_WHITELIST

        if not (is_mod or is_broadcaster or is_whitelisted):
            logger.debug("Ignored #okru from non-privileged user: %s", author.name)
            return

        now = time.monotonic()

        if self._checking:
            elapsed = now - self._check_start
            if elapsed < _DEBOUNCE_WINDOW:
                logger.info(
                    "Debounce: in progress (%.1fs elapsed), ignoring from %s",
                    elapsed, author.name,
                )
                return
            self._checking = False

        self._checking = True
        self._check_start = now

        channel = message.channel
        await self._safe_send(
            channel,
            "👀 Buscando streaming... espera un momento PogChamp",
            emote_text="👀 PogChamp",
        )

        asyncio.create_task(self._do_check(channel))

    # ------------------------------------------------------------------
    # Check task
    # ------------------------------------------------------------------
    async def _do_check(self, channel: twitchio.Channel):
        from app.services.check_service import run_check

        try:
            found, sid = await asyncio.wait_for(run_check(), timeout=_DEBOUNCE_WINDOW)
            if found:
                msg = "✅ Stream activo detectado! En unos momentos se actualizará el sitio"
                emote = "✅ PogChamp"
            else:
                msg = "📭 No hay stream activo en este momento BibleThump"
                emote = "📭 BibleThump"
            await self._safe_send(channel, msg, emote_text=emote)
        except asyncio.TimeoutError:
            logger.warning("Check timed out after %ss", _DEBOUNCE_WINDOW)
            await self._safe_send(channel, "⏱️ La búsqueda tardó demasiado, intenta de nuevo.", emote_text="⏱️")
        except Exception as e:
            logger.error("Check error: %s", e)
            await self._safe_send(channel, "❌ Error durante la búsqueda NotLikeThis", emote_text="NotLikeThis")
        finally:
            self._checking = False

    async def event_error(self, error: Exception, data: str | None = None):
        logger.error("Bot error: %s", error)


async def stop_bot():
    global _bot_instance
    if _bot_instance is not None:
        try:
            await _bot_instance.close()
        except Exception as e:
            logger.warning("Error closing bot: %s", e)
        _bot_instance = None


async def start_bot():
    global _bot_instance
    await stop_bot()
    creds = load_credentials()
    if not creds:
        logger.warning("No Twitch credentials — bot not started")
        return

    access_token = creds.get("access_token")
    if not access_token:
        logger.error("Credentials missing access_token")
        return

    # twitchio v2 expects token without "oauth:" prefix in some cases,
    # but adding it is safe — the library handles both.
    token = access_token
    if not token.startswith("oauth:"):
        token = f"oauth:{token}"

    _bot_instance = OkruBot(token=token)
    try:
        await _bot_instance.start()
    except Exception as e:
        logger.error("Bot crashed: %s", e)
        _bot_instance = None


def get_bot() -> "OkruBot | None":
    return _bot_instance
