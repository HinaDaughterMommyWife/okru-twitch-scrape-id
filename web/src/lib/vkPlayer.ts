type VkPlayer = {
  unmute: () => void;
  on: (event: string, listener: (state: unknown) => void) => void;
  destroy: () => void;
};

type VkApi = {
  VideoPlayer: (frame: HTMLIFrameElement) => VkPlayer;
};

declare global {
  interface Window {
    VK?: VkApi;
  }
}

const API_SRC = "https://vk.com/js/api/videoplayer.js";
const bound = new WeakSet<HTMLIFrameElement>();

let apiLoading: Promise<VkApi["VideoPlayer"]> | null = null;

function loadVideoPlayer(): Promise<VkApi["VideoPlayer"]> {
  if (window.VK?.VideoPlayer) return Promise.resolve(window.VK.VideoPlayer);
  if (apiLoading) return apiLoading;

  apiLoading = new Promise((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>("script[data-vk-player-api]");
    const onReady = () => {
      const create = window.VK?.VideoPlayer;
      if (create) resolve(create);
      else reject(new Error("VK.VideoPlayer missing"));
    };

    if (existing) {
      existing.addEventListener("load", onReady, { once: true });
      existing.addEventListener("error", () => reject(new Error("VK player script failed")), { once: true });
      return;
    }

    const script = document.createElement("script");
    script.src = API_SRC;
    script.async = true;
    script.dataset.vkPlayerApi = "";
    script.addEventListener("load", onReady, { once: true });
    script.addEventListener("error", () => reject(new Error("VK player script failed")), { once: true });
    document.head.appendChild(script);
  });

  return apiLoading;
}

function unmute(player: VkPlayer): void {
  try {
    player.unmute();
  } catch {
    // VK player not ready yet
  }
}

function unmuteOnGesture(player: VkPlayer): void {
  const run = () => unmute(player);
  window.addEventListener("pointerdown", run, { capture: true, once: true });
  window.addEventListener("keydown", run, { capture: true, once: true });
}

export function attachVkSound(frame: HTMLIFrameElement): void {
  const src = frame.getAttribute("src");
  if (!src || src.startsWith("about:") || bound.has(frame)) return;
  bound.add(frame);

  void loadVideoPlayer()
    .then((VideoPlayer) => {
      if (!frame.isConnected) return;
      const player = VideoPlayer(frame);
      unmute(player);
      player.on("inited", () => unmute(player));
      player.on("started", () => unmute(player));
      player.on("autoplaySoundProhibited", () => unmuteOnGesture(player));
    })
    .catch(() => {
      bound.delete(frame);
    });
}
