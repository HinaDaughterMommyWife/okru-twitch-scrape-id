export interface StreamingData {
  streaming_id: string;
  updated_at: string;
}

export interface StreamingError {
  error: string;
}

export type StreamingResult =
  | { ok: true; data: StreamingData }
  | { ok: false; error: string };

export async function fetchStreamingId(
  workerUrl: string,
  authToken: string
): Promise<StreamingResult> {
  const credentials = btoa(`admin:${authToken}`);

  const res = await fetch(`${workerUrl}/streaming`, {
    headers: {
      Authorization: `Basic ${credentials}`,
    },
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
