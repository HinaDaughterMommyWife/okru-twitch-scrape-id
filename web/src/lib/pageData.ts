import { env } from "cloudflare:workers";
import {
  basePath,
  fetchUser,
  fetchVods,
  liveIds,
  parseVideoQueryId,
  resolveVodQuery,
  type UserProfile,
  type VodItem,
} from "./streaming";

/** Only KV caches data — pages are never cached by the CDN. */
export const NO_STORE = "private, no-store";

const CONFIG_ERROR = "Configuración incompleta, comuníquese con el administrador.";

export function workerEnv() {
  return {
    workerUrl: env.WORKER_URL ?? "",
    authToken: env.WORKER_AUTH_TOKEN ?? "",
    defaultUser: (env.DEFAULT_USER ?? "").trim().toLowerCase(),
  };
}

/**
 * Canonical slug for `/[user]` routes.
 * - `redirect`: default user (lives at `/`) or non-lowercase slug
 * - `slug`: render this user
 */
export function resolveUserParam(
  param: string | undefined,
  url: URL,
  suffix: "" | "/vods",
): { redirect: string } | { slug: string } {
  const raw = param ?? "";
  const slug = raw.toLowerCase();
  const { defaultUser } = workerEnv();
  if (slug === defaultUser) return { redirect: `${suffix || "/"}${url.search}` };
  if (slug !== raw) return { redirect: `/${slug}${suffix}${url.search}` };
  return { slug };
}

export type PageError = { kind: "notFound" } | { kind: "error"; message: string };

export type WatchPageData =
  | PageError
  | {
      kind: "ok";
      user: UserProfile;
      base: string;
      vkOid: string | null;
      vkId: string | null;
    };

export async function loadWatch(slug: string, url: URL): Promise<WatchPageData> {
  const { workerUrl, authToken, defaultUser } = workerEnv();
  if (!workerUrl || !authToken || !defaultUser) return { kind: "error", message: CONFIG_ERROR };

  const result = await fetchUser(workerUrl, authToken, slug);
  if (!result.ok) {
    return result.notFound ? { kind: "notFound" } : { kind: "error", message: result.error };
  }

  const ids = parseVideoQueryId(url.searchParams.get("id")) ?? liveIds(result.data.streaming);
  return {
    kind: "ok",
    user: result.data.user,
    base: basePath(slug, defaultUser),
    vkOid: ids?.vkOid ?? null,
    vkId: ids?.vkId ?? null,
  };
}

export type VodsPageData =
  | PageError
  | {
      kind: "ok";
      user: UserProfile;
      base: string;
      items: VodItem[];
      playing: { vkOid: string; vkId: string } | null;
    };

export async function loadVods(slug: string, url: URL): Promise<VodsPageData> {
  const { workerUrl, authToken, defaultUser } = workerEnv();
  if (!workerUrl || !authToken || !defaultUser) return { kind: "error", message: CONFIG_ERROR };

  const [userResult, items] = await Promise.all([
    fetchUser(workerUrl, authToken, slug),
    fetchVods(workerUrl, authToken, slug).catch(() => [] as VodItem[]),
  ]);
  if (!userResult.ok) {
    return userResult.notFound ? { kind: "notFound" } : { kind: "error", message: userResult.error };
  }

  return {
    kind: "ok",
    user: userResult.data.user,
    base: basePath(slug, defaultUser),
    items,
    playing: resolveVodQuery(url.searchParams.get("id"), items),
  };
}
