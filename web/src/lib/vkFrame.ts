import { vkEmbedSrc } from "./streaming";
import { attachVkSound } from "./vkPlayer";

const FRAME_CLASS = "relative z-0 h-full w-full border-0 pointer-events-none";
const FRAME_ALLOW = "autoplay; encrypted-media; fullscreen; picture-in-picture; screen-wake-lock;";

function makeFrame(): HTMLIFrameElement {
  const frame = document.createElement("iframe");
  frame.title = "video";
  frame.dataset.vkFrame = "";
  frame.className = FRAME_CLASS;
  frame.width = "1920";
  frame.height = "1080";
  frame.allow = FRAME_ALLOW;
  frame.allowFullscreen = true;
  return frame;
}

export function swapVkFrame(pane: HTMLElement, oid: string, vid: string): HTMLIFrameElement {
  const current = pane.querySelector<HTMLIFrameElement>("[data-vk-frame]");
  const playing =
    current &&
    current.dataset.vkOid === oid &&
    current.dataset.vkVid === vid &&
    current.getAttribute("src") &&
    !current.src.startsWith("about:");
  if (current && playing) return current;

  const next = makeFrame();
  next.dataset.vkOid = oid;
  next.dataset.vkVid = vid;
  const hoverable = window.matchMedia("(hover: hover) and (pointer: fine)").matches;
  if (!hoverable) next.style.pointerEvents = "auto";
  else if (current?.style.pointerEvents) next.style.pointerEvents = current.style.pointerEvents;
  next.src = vkEmbedSrc(oid, vid);
  attachVkSound(next);

  if (current) current.replaceWith(next);
  else pane.prepend(next);
  return next;
}

export function unloadVkFrame(pane: HTMLElement): void {
  const current = pane.querySelector<HTMLIFrameElement>("[data-vk-frame]");
  if (!current) return;
  const next = makeFrame();
  next.style.pointerEvents = "none";
  current.replaceWith(next);
}
