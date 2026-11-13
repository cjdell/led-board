/// Live panel mirror: paints the board's real framebuffer (`GET /api/frame`,
/// raw RGB888 from the display worker) into a `LedMatrix` canvas, so the
/// "now playing" tiles show what the LEDs actually display.
///
/// The board publishes at ~4 fps; polling at 2 fps is plenty for the
/// dot-matrix content. When the feed goes unavailable (board rebooting, old
/// firmware, radio link down) polling backs off and `onLive(false)` lets the
/// component run the local wasm preview until the next probe succeeds.

import { PANEL_FRAME_BYTES, PANEL_HEIGHT, PANEL_WIDTH } from "./device/common.ts";

const POLL_MS = 500;
const PROBE_MS = 4_000;

export interface LiveMirrorOptions {
  fetchFrame: () => Promise<Uint8Array | null>;
  onLive: (live: boolean) => void;
}

export function startLiveMirror(canvas: HTMLCanvasElement, opts: LiveMirrorOptions): () => void {
  const off = document.createElement("canvas");

  off.width = PANEL_WIDTH;
  off.height = PANEL_HEIGHT;

  const offCtx = off.getContext("2d")!;
  const image = offCtx.createImageData(PANEL_WIDTH, PANEL_HEIGHT);

  // Same target geometry as startPreview() so the dot-mask CSS lines up.
  canvas.width = PANEL_WIDTH * 4;
  canvas.height = PANEL_HEIGHT * 4;

  let live = false;
  let busy = false;
  let stopped = false;

  function draw(rgb: Uint8Array) {
    const data = image.data;

    for (let i = 0; i < PANEL_FRAME_BYTES / 3; i++) {
      const p = i * 4;
      const s = i * 3;

      data[p] = rgb[s];
      data[p + 1] = rgb[s + 1];
      data[p + 2] = rgb[s + 2];
      data[p + 3] = 255;
    }

    offCtx.putImageData(image, 0, 0);

    const ctx = canvas.getContext("2d")!;

    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(off, 0, 0, canvas.width, canvas.height);
  }

  function setLive(next: boolean) {
    if (live === next) return;

    live = next;

    clearInterval(timer);
    timer = setInterval(poll, live ? POLL_MS : PROBE_MS);

    opts.onLive(live);
  }

  async function poll() {
    if (busy || stopped) return;

    busy = true;

    try {
      const rgb = await opts.fetchFrame();

      if (stopped) return;

      if (rgb) {
        draw(rgb);
        setLive(true);
      } else {
        setLive(false);
      }
    } finally {
      busy = false;
    }
  }

  let timer = setInterval(poll, POLL_MS);

  poll();

  return () => {
    stopped = true;

    clearInterval(timer);
  };
}
