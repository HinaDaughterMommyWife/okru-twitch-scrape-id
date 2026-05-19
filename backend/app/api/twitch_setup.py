"""
Twitch OAuth setup routes.
POST /{TWITCH_SETUP_PATH_KEY}/setup  — register app credentials (one-time)
GET  /{TWITCH_SETUP_PATH_KEY}/oauth  — redirect to Twitch OAuth
GET  /{TWITCH_SETUP_PATH_KEY}/callback — handle OAuth callback
"""

import asyncio
import secrets
import urllib.parse

import httpx
from fastapi import APIRouter, HTTPException, Request, BackgroundTasks
from fastapi.responses import HTMLResponse, RedirectResponse

from app.core.config import settings
from app.services.credentials import credentials_exist, save_credentials, load_credentials

router = APIRouter()

_oauth_state: str | None = None


def _make_router():
    key = settings.TWITCH_SETUP_PATH_KEY

    @router.get(f"/{key}/setup", response_class=HTMLResponse)
    async def setup_page():
        if credentials_exist():
            raise HTTPException(status_code=409, detail="Bot credentials already configured")
        html = """
        <!DOCTYPE html><html><head><title>Okru Bot Setup</title></head>
        <body style="font-family:monospace;max-width:600px;margin:4rem auto;padding:1rem">
        <h2>🤖 Okru Twitch Bot — Setup</h2>
        <p>Credentials not yet configured.</p>
        <a href="./oauth" style="display:inline-block;padding:.6rem 1.2rem;background:#9147ff;color:#fff;text-decoration:none;border-radius:4px">
          Autorizar con Twitch
        </a>
        </body></html>
        """
        return HTMLResponse(html)

    @router.get(f"/{key}/oauth")
    async def oauth_redirect():
        if credentials_exist():
            raise HTTPException(status_code=409, detail="Already configured")
        global _oauth_state
        _oauth_state = secrets.token_urlsafe(32)
        params = urllib.parse.urlencode({
            "client_id": settings.TWITCH_CLIENT_ID,
                "redirect_uri": f"{settings.BASE_URL}/{key}/callback",
            "response_type": "code",
            "scope": "chat:read chat:edit user:bot user:read:chat user:write:chat",
            "state": _oauth_state,
            "force_verify": "true",
        })
        return RedirectResponse(f"https://id.twitch.tv/oauth2/authorize?{params}")

    @router.get(f"/{key}/callback")
    async def oauth_callback(code: str, state: str, background_tasks: BackgroundTasks):
        global _oauth_state
        if state != _oauth_state:
            raise HTTPException(status_code=400, detail="Invalid OAuth state")
        _oauth_state = None

        async with httpx.AsyncClient() as client:
            resp = await client.post("https://id.twitch.tv/oauth2/token", data={
                "client_id": settings.TWITCH_CLIENT_ID,
                "client_secret": settings.TWITCH_CLIENT_SECRET,
                "code": code,
                "grant_type": "authorization_code",
            "redirect_uri": f"{settings.BASE_URL}/{key}/callback",
            })
        if resp.status_code != 200:
            raise HTTPException(status_code=502, detail=f"Twitch token exchange failed: {resp.text}")

        data = resp.json()
        save_credentials(data)

        # Start bot in background
        background_tasks.add_task(_launch_bot)

        html = """
        <!DOCTYPE html><html><head><title>Setup Complete</title></head>
        <body style="font-family:monospace;max-width:600px;margin:4rem auto;padding:1rem">
        <h2>✅ Bot autorizado y configurado</h2>
        <p>El bot de Twitch está ahora activo. Puedes cerrar esta ventana.</p>
        </body></html>
        """
        return HTMLResponse(html)

    @router.get(f"/{key}/clear", response_class=HTMLResponse)
    async def clear_credentials_endpoint():
        from app.services.credentials import clear_credentials
        from app.bot.twitch_bot import stop_bot
        await stop_bot()
        clear_credentials()
        return HTMLResponse("<html><body style='font-family:monospace;max-width:600px;margin:4rem auto'><h2>✅ Credenciales eliminadas</h2><p><a href='./setup'>Volver a configurar</a></p></body></html>")

    return router


async def _launch_bot():
    from app.bot.twitch_bot import start_bot
    await start_bot()


_make_router()
