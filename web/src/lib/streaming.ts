export interface StreamingData {
  vk_oid: string;
  vk_id: string;
  updated_at: string;
}

export interface StreamingError {
  error: string;
}

export type StreamingResult =
  | { ok: true; data: StreamingData }
  | { ok: false; error: string };

export interface UserProfile {
  slug: string;
  display_name: string;
  twitch_channel: string;
  idvk: string;
  command: string;
  updated_at: string;
}

export interface UserData {
  user: UserProfile;
  streaming: StreamingData | null;
}

export type UserResult =
  | { ok: true; data: UserData }
  | { ok: false; notFound: boolean; error: string };

export interface VodItem {
  id: number;
  owner_id: number;
  title: string;
  duration: string;
  live_status: string;
  date: number;
  thumb: string;
}

export interface VodsData {
  items: VodItem[];
  updated_at: string;
}

export function vkEmbedSrc(vkOid: string, vkId: string): string {
  return `https://vk.com/video_ext.php?oid=${encodeURIComponent(vkOid)}&id=${encodeURIComponent(vkId)}&hd=4&autoplay=1&js_api=1`;
}

export function vkVideoWatchUrl(ownerId: number, id: number): string {
  return `https://vkvideo.ru/video${ownerId}_${id}`;
}

/** `""` for the default user (served at `/`), `/slug` for everyone else. */
export function basePath(slug: string, defaultUser: string): string {
  return slug === defaultUser ? "" : `/${slug}`;
}

export function vodsPath(base: string): string {
  return `${base}/vods`;
}

export function vodWatchPath(base: string, ownerId: string | number, id: string | number): string {
  return `${vodsPath(base)}?id=${ownerId}_${id}`;
}

export function resolveVodQuery(
  queryId: string | null,
  items: VodItem[],
): { vkOid: string; vkId: string } | null {
  const parsed = parseVideoQueryId(queryId);
  if (parsed) return parsed;
  if (queryId && /^-?\d+$/.test(queryId)) {
    const item = items.find((entry) => String(entry.id) === queryId);
    if (item) return { vkOid: String(item.owner_id), vkId: String(item.id) };
  }
  return null;
}

export function parseVideoQueryId(
  queryId: string | null,
): { vkOid: string; vkId: string } | null {
  const idMatch = queryId?.match(/^(?:video[_-]?)?(-?\d+)_(\d+)$/);
  if (!idMatch) return null;
  return { vkOid: idMatch[1], vkId: idMatch[2] };
}

export function liveStreamIds(
  result: StreamingResult,
): { vkOid: string; vkId: string } | null {
  if (!result.ok) return null;
  return liveIds(result.data);
}

export function liveIds(
  data: StreamingData | null,
): { vkOid: string; vkId: string } | null {
  if (!data) return null;
  const { vk_oid, vk_id } = data;
  if (!vk_oid || !vk_id || vk_oid === "NOT_FOUND" || vk_id === "NOT_FOUND") return null;
  return { vkOid: vk_oid, vkId: vk_id };
}

function workerAuth(authToken: string): string {
  return `Basic ${btoa(`admin:${authToken}`)}`;
}

function userUrl(workerUrl: string, slug: string, suffix = ""): string {
  return `${workerUrl}/users/${encodeURIComponent(slug)}${suffix}`;
}

function workerFetch(url: string, authToken: string): Promise<Response> {
  return fetch(url, {
    headers: { Authorization: workerAuth(authToken) },
    cache: "no-store",
  });
}

async function errorOf(res: Response): Promise<string> {
  const body = (await res.json().catch(() => null)) as StreamingError | null;
  return body?.error ?? `HTTP ${res.status}`;
}

/** Profile + current live ids in one worker call. */
export async function fetchUser(
  workerUrl: string,
  authToken: string,
  slug: string,
): Promise<UserResult> {
  const res = await workerFetch(userUrl(workerUrl, slug), authToken);
  if (!res.ok) {
    return { ok: false, notFound: res.status === 404 || res.status === 400, error: await errorOf(res) };
  }
  return { ok: true, data: (await res.json()) as UserData };
}

export async function fetchStreamingId(
  workerUrl: string,
  authToken: string,
  slug: string,
): Promise<StreamingResult> {
  const res = await workerFetch(userUrl(workerUrl, slug, "/streaming"), authToken);
  if (!res.ok) return { ok: false, error: await errorOf(res) };
  return { ok: true, data: (await res.json()) as StreamingData };
}

export async function fetchVods(
  workerUrl: string,
  authToken: string,
  slug: string,
): Promise<VodItem[]> {
  const res = await workerFetch(userUrl(workerUrl, slug, "/vods"), authToken);
  if (!res.ok) return [];

  const data = (await res.json()) as VodsData | VodItem[];
  if (Array.isArray(data)) return data;
  if (Array.isArray(data.items)) return data.items;
  return [];
}
