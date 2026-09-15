export const prerender = false;

import type { APIRoute } from "astro";
import { fetchStreamingId } from "../../lib/streaming";
import { NO_STORE, workerEnv } from "../../lib/pageData";

const headers = {
  "Content-Type": "application/json",
  "Cache-Control": NO_STORE,
};

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), { status, headers });
}

/** `GET /api/lastStream?user=<slug>` — defaults to DEFAULT_USER. */
export const GET: APIRoute = async ({ url }) => {
  const { workerUrl, authToken, defaultUser } = workerEnv();

  if (!workerUrl || !authToken || !defaultUser) {
    return json({ ok: false, error: "Configuración incompleta, comuníquese con el administrador." }, 503);
  }

  const slug = (url.searchParams.get("user") || defaultUser).toLowerCase();
  if (!/^[a-z0-9_-]{1,32}$/.test(slug)) {
    return json({ ok: false, error: "invalid user" }, 400);
  }

  return json(await fetchStreamingId(workerUrl, authToken, slug));
};
