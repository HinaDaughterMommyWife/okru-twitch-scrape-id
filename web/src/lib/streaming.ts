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

export function vodWatchPath(ownerId: string | number, id: string | number): string {
  return `/vods?id=${ownerId}_${id}`;
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
  const { vk_oid, vk_id } = result.data;
  if (!vk_oid || !vk_id || vk_oid === "NOT_FOUND" || vk_id === "NOT_FOUND") return null;
  return { vkOid: vk_oid, vkId: vk_id };
}

function workerAuth(authToken: string): string {
  return `Basic ${btoa(`admin:${authToken}`)}`;
}

export async function fetchStreamingId(
  workerUrl: string,
  authToken: string,
): Promise<StreamingResult> {
  const res = await fetch(`${workerUrl}/streaming`, {
    headers: {
      Authorization: workerAuth(authToken),
    },
    cache: "no-store",
  });

  if (!res.ok) {
    const body = (await res.json().catch(() => null)) as StreamingError | null;
    return {
      ok: false,
      error: body?.error ?? `HTTP ${res.status}`,
    };
  }

  const data = (await res.json()) as StreamingData;
  return { ok: true, data };
}

export async function fetchVods(
  workerUrl: string,
  authToken: string,
): Promise<VodItem[]> {
  const res = await fetch(`${workerUrl}/vods`, {
    headers: {
      Authorization: workerAuth(authToken),
    },
    cache: "no-store",
  });

  if (!res.ok) return [];

  const data = (await res.json()) as VodsData | VodItem[];
  if (Array.isArray(data)) return data;
  if (Array.isArray(data.items)) return data.items;
  return [];
}
