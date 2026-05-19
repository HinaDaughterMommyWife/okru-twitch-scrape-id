"""
Twitch bot via twitchio v3 (EventSub WebSocket).
- Listens for chat messages via eventsub channel.chat.message
- Handles #okru from mods/broadcaster/whitelist
- Undebounce: first call fires, subsequent ignored until done or 40s
- Handles: emote-only, slow mode, chat-disabled silently
"""

import asyncio
import logging
import time

import twitchio

from app.core.config import settings
from app.services.credentials import load_credentials

logger = logging.getLogger("okru.bot")

_SLOW_MODE_DEFAULT = 5.0
_DEBOUNCE_WINDOW = 40.0

_bot_instance: "OkruBot | None" = None


class OkruBot(twitchio.Client):
    def __init__(self, access_token: str, refresh_token: str, bot_id: str, broadcaster_id: str):
        super().__init__(
            client_id=settings.TWITCH_CLIENT_ID,
            client_secret=settings.TWITCH_CLIENT_SECRET,
        )
        self._access_token = access_token
        self._refresh_token = refresh_token
        self._bot_id = bot_id
        self._broadcaster_id = broadcaster_id

        self._checking = False
        self._check_start: float = 0.0
        self._slow_mode: float = _SLOW_MODE_DEFAULT
        self._emote_only: bool = False
        self._chat_disabled: bool = False
        self._last_msg_time: float = 0.0

    # ------------------------------------------------------------------
    # Setup
    # ------------------------------------------------------------------
    async def setup_hook(self):
        await self.add_token(self._access_token, self._refresh_token)

        await self.subscribe_websocket(
            payload=twitchio.eventsub.ChatMessageSubscription(
                broadcaster_user_id=self._broadcaster_id,
                user_id=self._bot_id,
            ),
            token_for=self._bot_id,
        )

        # Subscribe to chat settings updates for emote-only / slow mode
        await self.subscribe_websocket(
            payload=twitchio.eventsub.ChatSettingsUpdateSubscription(
                broadcaster_user_id=self._broadcaster_id,
                user_id=self._bot_id,
            ),
            token_for=self._bot_id,
        )

        logger.info("Bot setup complete — listening on channel %s", settings.TWITCH_CHANNEL)

    # ------------------------------------------------------------------
    # Chat settings (emote-only, slow mode)
    # ------------------------------------------------------------------
    async def event_chat_settings_update(self, data: twitchio.ChatSettingsUpdate):
        try:
            self._emote_only = getattr(data, "emote_mode", False) or False
            slow = getattr(data, "slow_mode_wait_time", None)
            if slow and slow > 0:
                self._slow_mode = max(float(slow), _SLOW_MODE_DEFAULT)
            else:
                self._slow_mode = _SLOW_MODE_DEFAULT

            # subscriber_mode = effectively chat disabled for non-subs (bot may not be sub)
            sub_mode = getattr(data, "subscriber_mode", False) or False
            follower_only = getattr(data, "follower_mode", False) or False
            self._chat_disabled = bool(sub_mode or follower_only)

            logger.info(
                "Chat settings: emote_only=%s slow=%.0fs chat_disabled=%s",
                self._emote_only, self._slow_mode, self._chat_disabled,
            )
        except Exception as e:
            logger.warning("ChatSettingsUpdate parse error (non-fatal): %s", e)

    # ------------------------------------------------------------------
    # Safe send
    # ------------------------------------------------------------------
    async def _safe_send(self, text: str, emote_text: str | None = None):
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
            users = await self.fetch_users(ids=[self._broadcaster_id], token_for=self._access_token)
            if users:
                await users[0].send_message(
                    message=msg,
                    sender=self._bot_id,
                    token_for=self._access_token,
                )
            self._last_msg_time = time.monotonic()
            logger.info("Sent: %s", msg)
        except Exception as e:
            logger.warning("Send failed (non-fatal): %s", e)

    # ------------------------------------------------------------------
    # Message event
    # ------------------------------------------------------------------
    async def event_message(self, data: twitchio.ChatMessage):
        text = (data.text or "").strip().lower()
        if text != "#okru":
            return

        chatter = data.chatter
        if not chatter:
            return

        is_mod = getattr(chatter, "moderator", False) or False
        is_broadcaster = getattr(chatter, "broadcaster", False) or False
        is_whitelisted = (chatter.name or "").lower() in settings.TWITCH_WHITELIST

        if not (is_mod or is_broadcaster or is_whitelisted):
            logger.debug("Ignored #okru from non-privileged user: %s", chatter.name)
            return

        now = time.monotonic()

        if self._checking:
            elapsed = now - self._check_start
            if elapsed < _DEBOUNCE_WINDOW:
                logger.info(
                    "Debounce: in progress (%.1fs elapsed), ignoring from %s",
                    elapsed, chatter.name,
                )
                return
            self._checking = False

        self._checking = True
        self._check_start = now

        await self._safe_send(
            "👀 Buscando streaming... espera un momento PogChamp",
            emote_text="👀 PogChamp",
        )

        asyncio.create_task(self._do_check())

    # ------------------------------------------------------------------
    # Check task
    # ------------------------------------------------------------------
    async def _do_check(self):
        from app.services.check_service import run_check

        try:
            found, sid = await asyncio.wait_for(run_check(), timeout=_DEBOUNCE_WINDOW)
            if found:
                msg = f"✅ Stream activo detectado! En unos momentos se actualizará el sitio"
                emote = "✅ PogChamp"
            else:
                msg = "📭 No hay stream activo en este momento BibleThump"
                emote = "📭 BibleThump"
            await self._safe_send(msg, emote_text=emote)
        except asyncio.TimeoutError:
            logger.warning("Check timed out after %ss", _DEBOUNCE_WINDOW)
            await self._safe_send("⏱️ La búsqueda tardó demasiado, intenta de nuevo.", emote_text="⏱️")
        except Exception as e:
            logger.error("Check error: %s", e)
            await self._safe_send("❌ Error durante la búsqueda NotLikeThis", emote_text="NotLikeThis")
        finally:
            self._checking = False

    async def event_token_refreshed(self, payload: twitchio.TokenRefreshedPayload):
        try:
            creds = load_credentials() or {}
            creds["access_token"] = payload.token
            creds["refresh_token"] = payload.refresh_token
            from app.services.credentials import save_credentials
            save_credentials(creds)
            logger.info("Token refreshed and persisted for user %s", payload.user_id)
        except Exception as e:
            logger.warning("Failed to persist refreshed token: %s", e)

    async def event_error(self, error: Exception, *args, **kwargs):
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

    # Resolve bot user ID and broadcaster ID via raw HTTP (token not yet in client)
    try:
        import httpx
        headers = {
            "Authorization": f"Bearer {access_token}",
            "Client-Id": settings.TWITCH_CLIENT_ID,
        }
        async with httpx.AsyncClient() as http:
            r = await http.get(
                "https://api.twitch.tv/helix/users",
                params={"login": [settings.TWITCH_BOT_USERNAME, settings.TWITCH_CHANNEL]},
                headers=headers,
            )
            r.raise_for_status()
            users_data = {u["login"].lower(): u["id"] for u in r.json()["data"]}

        bot_id = users_data.get(settings.TWITCH_BOT_USERNAME.lower())
        broadcaster_id = users_data.get(settings.TWITCH_CHANNEL.lower())

        if not bot_id or not broadcaster_id:
            logger.error("Could not resolve IDs: %s", users_data)
            return

        bot_id = str(bot_id)
        broadcaster_id = str(broadcaster_id)
    except Exception as e:
        logger.error("Failed to resolve user IDs: %s", e)
        return

    _bot_instance = OkruBot(
        access_token=access_token,
        refresh_token=creds.get("refresh_token", ""),
        bot_id=bot_id,
        broadcaster_id=broadcaster_id,
    )
    try:
        await _bot_instance.start()
    except Exception as e:
        logger.error("Bot crashed: %s", e)
        _bot_instance = None


def get_bot() -> "OkruBot | None":
    return _bot_instance
