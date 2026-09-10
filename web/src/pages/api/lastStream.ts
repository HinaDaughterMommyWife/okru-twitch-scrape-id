export const prerender = false;

import type { APIRoute } from "astro";
import { env } from "cloudflare:workers";
import { fetchStreamingId } from "../../lib/streaming";

const CACHE_CONTROL = "public, s-maxage=5";

export const GET: APIRoute = async () => {
  const workerUrl = env.WORKER_URL ?? "";
  const authToken = env.WORKER_AUTH_TOKEN ?? "";

  const headers = {
    "Content-Type": "application/json",
    "Cache-Control": CACHE_CONTROL,
  };

  if (!workerUrl || !authToken) {
    return new Response(
      JSON.stringify({
        ok: false,
        error: "Configuración incompleta, comuníquese con el administrador.",
      }),
      { status: 503, headers },
    );
  }

  const result = await fetchStreamingId(workerUrl, authToken);
  return new Response(JSON.stringify(result), {
    status: 200,
    headers,
  });
};
