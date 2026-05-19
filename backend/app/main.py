from fastapi import FastAPI
from fastapi.responses import JSONResponse
from contextlib import asynccontextmanager
import asyncio
import logging

from app.api.twitch_setup import router as twitch_router
from app.services.credentials import credentials_exist

logging.basicConfig(level=logging.INFO, format="%(asctime)s %(name)s %(levelname)s %(message)s")
logger = logging.getLogger("okru")


@asynccontextmanager
async def lifespan(app: FastAPI):
    if credentials_exist():
        logger.info("Credentials found — starting Twitch bot...")
        from app.bot.twitch_bot import start_bot
        asyncio.create_task(start_bot())
    else:
        logger.info("No credentials — visit /{TWITCH_SETUP_PATH_KEY}/setup to configure bot")
    yield
    from app.bot.twitch_bot import stop_bot
    await stop_bot()


app = FastAPI(title="okru-backend", lifespan=lifespan)
app.include_router(twitch_router)


@app.get("/health")
async def health():
    return JSONResponse({"status": "ok", "bot_active": credentials_exist()})

