const IDLE_MS = 500;

function canHover(): boolean {
  return window.matchMedia("(hover: hover) and (pointer: fine)").matches;
}

function badgesIn(pane: HTMLElement): HTMLButtonElement[] {
  return [...pane.querySelectorAll<HTMLButtonElement>("[data-player-badge]")];
}

function frameIn(pane: HTMLElement): HTMLElement | null {
  const el = pane.querySelector("[data-vk-frame]");
  return el instanceof HTMLElement ? el : null;
}

export function initPlayerOverlays(): void {
  for (const pane of document.querySelectorAll<HTMLElement>("[data-video-pane]")) {
    if (pane.dataset.playerOverlay === "on") continue;
    pane.dataset.playerOverlay = "on";
    bindPane(pane);
  }
}

function bindPane(pane: HTMLElement): void {
  const stage = pane.closest("[data-stream-player]");
  const hoverable = canHover();
  let hideTimer = 0;

  const isWaiting = () => stage?.getAttribute("data-mode") === "waiting";

  const armFrameHits = () => {
    const frame = frameIn(pane);
    if (frame) frame.style.pointerEvents = "none";
  };

  const passFrameHits = () => {
    const frame = frameIn(pane);
    if (frame) frame.style.pointerEvents = "auto";
  };

  const setChrome = (show: boolean) => {
    for (const badge of badgesIn(pane)) {
      badge.style.opacity = show ? "1" : "0";
      badge.style.pointerEvents = show ? "auto" : "none";
    }
  };

  const showChrome = () => {
    if (isWaiting()) return;
    window.clearTimeout(hideTimer);
    setChrome(true);
    passFrameHits();
  };

  const hideChrome = () => {
    setChrome(false);
    armFrameHits();
  };

  const hideChromeSoon = () => {
    window.clearTimeout(hideTimer);
    hideTimer = window.setTimeout(hideChrome, IDLE_MS);
  };

  if (hoverable) {
    const onMove = () => {
      showChrome();
      hideChromeSoon();
    };
    pane.addEventListener("mousemove", onMove);
    pane.addEventListener("mouseenter", onMove);
    pane.addEventListener("mouseleave", hideChromeSoon);
    hideChrome();
  } else {
    showChrome();
  }

  pane.addEventListener("click", (event) => {
    const badge = (event.target as Element | null)?.closest?.("[data-player-badge]");
    if (!(badge instanceof HTMLButtonElement) || !pane.contains(badge)) return;

    const attr = badge.getAttribute("data-toggle-attr");
    const rootSel = badge.getAttribute("data-toggle-root");
    if (attr && rootSel) {
      const root = pane.closest(rootSel);
      if (root instanceof HTMLElement) {
        const on = root.toggleAttribute(attr);
        const label = badge.getAttribute("data-label") ?? "";
        const alt = badge.getAttribute("data-alt-label") ?? label;
        badge.textContent = on ? alt : label;
      }
    }

    if (hoverable) {
      showChrome();
      hideChromeSoon();
    }
  });
}

export function resetPlayerBadges(root: Element): void {
  for (const badge of root.querySelectorAll<HTMLButtonElement>("[data-player-badge]")) {
    const label = badge.getAttribute("data-label");
    if (label) badge.textContent = label;
  }
}
