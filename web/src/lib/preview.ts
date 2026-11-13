/// Preview renderer: draws every `AnimationEnum` into a 60×40 canvas using
/// the firmware's own renderers — the `anim_wasm` crate (repo root) compiled
/// to WebAssembly and base64-inlined by `web/embed_wasm.ts`. There is no
/// TypeScript animation code any more: a preview card literally runs the
/// Rust the panel runs, so previews and hardware cannot drift.
///
/// One shared rAF loop drives every registered preview at its own frame
/// budget (hero 24 fps, library cards ~8 fps). Each tick hands the
/// animation's board wire format to `anim_render`, then blits the returned
/// RGB888 frame — byte-identical layout to `GET /api/frame` — into the
/// canvas at full brightness (the panel's gamma stage exists to reproduce
/// exactly these colours on linear strips, and canvas already applies
/// sRGB — same reasoning as the sim).
///
/// Failure posture: if the engine fails to load or traps (a trap would be a
/// watchdog reset on the board — a firmware bug, not a preview bug), previews
/// hold a dark `Blank`-navy frame, the console says why, and the SPA itself
/// keeps working.
import { Animation, toast } from "@lib";
import { ANIM_WASM_BASE64 } from "./wasm_gen.ts";

export const PREVIEW_WIDTH = 60;
export const PREVIEW_HEIGHT = 40;

const FRAME_BYTES = PREVIEW_WIDTH * PREVIEW_HEIGHT * 3;

/// Mirrors `anim_wasm`'s `STATUS_BAD_JSON`.
const STATUS_BAD_JSON = 1;

/// A live 60×40 canvas: an offscreen 1:1 buffer (pixel-exact, upscaled with
/// nearest-neighbour) plus the wire form of the animation it shows.
interface Preview {
  canvas: HTMLCanvasElement;
  off: HTMLCanvasElement;
  offCtx: CanvasRenderingContext2D;
  image: ImageData;
  json: string;
  fps: number;
  last: number;
  start: number;
  /** Render-rejection messages are logged once per preview, not per frame. */
  complained: boolean;
}

// ── engine ────────────────────────────────────────────────────────────────

interface AnimWasm {
  memory: WebAssembly.Memory;
  anim_alloc: (len: number) => number;
  anim_free: (ptr: number, len: number) => void;
  anim_render: (jsonPtr: number, jsonLen: number, msSinceStart: number) => number;
  anim_frame_ptr: () => number;
  anim_error_ptr: () => number;
  anim_error_len: () => number;
}

let engine: AnimWasm | null = null;
let loading: Promise<AnimWasm | null> | null = null;

/** A failed *load* is permanent for this page (bad artifact); traps retry. */
let loadFailed = false;
let traps = 0;

/** Resolves null when the engine is permanently unusable (see dead path). */
function loadEngine(): Promise<AnimWasm | null> {
  if (engine) return Promise.resolve(engine);
  if (loadFailed) return Promise.resolve(null);
  if (!loading) loading = instantiate().finally(() => loading = null);

  return loading;
}

async function instantiate(): Promise<AnimWasm | null> {
  if (ANIM_WASM_BASE64.length === 0) {
    loadFailed = true;
    engineDead("no embedded wasm — run `deno task wasm` in web/ (or `just wasm`)");

    return null;
  }

  try {
    // Engines disagree on what `instantiate(bytes)` resolves to: the old
    // spec shape `{ module, instance }` (Deno; some browser paths) or a bare
    // `Instance` (what the TS DOM lib types). Accept either.
    type Loaded = WebAssembly.Instance | { instance: WebAssembly.Instance };
    const loaded = (await WebAssembly.instantiate(decodeBase64(ANIM_WASM_BASE64), {})) as unknown as Loaded;
    const instance = "instance" in loaded ? loaded.instance : loaded;
    const exports = instance.exports as Record<string, unknown>;

    for (const name of ["anim_alloc", "anim_free", "anim_render", "anim_frame_ptr", "memory"]) {
      if (exports[name] === undefined) throw new Error(`anim_wasm is missing \`${name}\``);
    }

    engine = instance.exports as unknown as AnimWasm;

    return engine;
  } catch (err) {
    loadFailed = true;
    engineDead(`instantiate failed: ${String(err)}`);

    return null;
  }
}

/// Give up on previews (after a couple of wasm traps) and tell the user.
function engineDead(reason: string) {
  engine = null;

  console.error(`animation preview engine disabled — ${reason}`);
  toast("Animation previews unavailable on this page.", "warn", 6_000);

  for (const preview of previews) blank(preview);
}

const decoder = new TextDecoder();

function engineError(w: AnimWasm): string {
  return decoder.decode(new Uint8Array(w.memory.buffer, w.anim_error_ptr(), w.anim_error_len()));
}

// ── frame loop ────────────────────────────────────────────────────────────

let loop = 0;
const previews = new Set<Preview>();

function frame(now: number) {
  if (!engine) {
    loop = 0; // the load attempt owns re-arming; rAF stops until then

    loadEngine().then((wasm) => {
      if (wasm && previews.size > 0) loop = requestAnimationFrame(frame);
    });

    return;
  }

  loop = requestAnimationFrame(frame);

  for (const preview of previews) {
    if (now - preview.last < 1000 / preview.fps) continue;

    preview.last = now;

    try {
      render(preview, (now - preview.start) >>> 0); // wraps like the board's u32 clock
    } catch (err) {
      // A wasm trap is a panic in shared firmware code: re-instantiate a
      // couple of times (the trap may be animation-specific), then stop.
      traps += 1;
      engine = null;

      console.error(`animation preview wasm trap${traps > 2 ? " (final)" : ""}`, err, preview.json);

      if (traps > 2) {
        loadFailed = true;
        engineDead("repeated wasm traps — the renderers panicked");
      }

      break;
    }
  }
}

const encoder = new TextEncoder();

function render(preview: Preview, tMs: number) {
  const w = engine!;
  const bytes = encoder.encode(preview.json);
  const ptr = w.anim_alloc(bytes.length);

  new Uint8Array(w.memory.buffer, ptr, bytes.length).set(bytes);

  try {
    renderInto(preview, w.anim_render(ptr, bytes.length, tMs), w);
  } finally {
    w.anim_free(ptr, bytes.length);
  }
}

function renderInto(preview: Preview, status: number, w: AnimWasm) {
  if (status === STATUS_BAD_JSON) {
    if (!preview.complained) {
      preview.complained = true;

      console.warn(`preview: board rejected animation ${preview.json}: ${engineError(w)}`);
    }

    blank(preview);

    return;
  }

  if (status !== 0) {
    if (!preview.complained) {
      preview.complained = true;

      console.warn(`preview: anim_render returned status ${status} for ${preview.json}`);
    }

    blank(preview);

    return;
  }

  // Views into wasm memory are rebuilt every frame: allocation can grow
  // linear memory at any render and detach previous ones.
  const src = new Uint8Array(w.memory.buffer, w.anim_frame_ptr(), FRAME_BYTES);
  const d = preview.image.data;

  for (let i = 0, s = 0; s < FRAME_BYTES;) {
    d[i] = src[s];
    d[i + 1] = src[s + 1];
    d[i + 2] = src[s + 2];
    d[i + 3] = 255;
    i += 4;
    s += 3;
  }

  blit(preview);
}

/** The firmware `Blank` fill — the honest "nothing rendered" frame. */
function blank(preview: Preview) {
  const d = preview.image.data;

  for (let i = 0; i < d.length; i += 4) {
    d[i] = 0;
    d[i + 1] = 0;
    d[i + 2] = 0x10;
    d[i + 3] = 255;
  }

  blit(preview);
}

function blit(preview: Preview) {
  preview.offCtx.putImageData(preview.image, 0, 0);

  const ctx = preview.canvas.getContext("2d")!;

  ctx.imageSmoothingEnabled = false;
  ctx.drawImage(preview.off, 0, 0, preview.canvas.width, preview.canvas.height);
}

function decodeBase64(b64: string): Uint8Array {
  const bin = atob(b64);
  const bytes = new Uint8Array(bin.length);

  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);

  return bytes;
}

// ── lifecycle ─────────────────────────────────────────────────────────────

export function startPreview(canvas: HTMLCanvasElement, animation: Animation, fps = 24): () => void {
  const off = document.createElement("canvas");

  off.width = PREVIEW_WIDTH;
  off.height = PREVIEW_HEIGHT;

  const offCtx = off.getContext("2d")!;

  canvas.width = PREVIEW_WIDTH * 4;
  canvas.height = PREVIEW_HEIGHT * 4;

  const preview: Preview = {
    canvas,
    off,
    offCtx,
    image: offCtx.createImageData(PREVIEW_WIDTH, PREVIEW_HEIGHT),
    // The `Animation` UI value *is* the `AnimationEnum` serde JSON wire form
    // the board's endpoints and playlist files use — wasm parses the same.
    json: JSON.stringify(animation),
    fps,
    last: 0,
    start: performance.now(),
    complained: false,
  };

  previews.add(preview);

  if (engine) {
    if (!loop) loop = requestAnimationFrame(frame);
  } else {
    loadEngine().then((wasm) => {
      if (wasm && previews.size > 0 && !loop) loop = requestAnimationFrame(frame);
      if (!wasm) blank(preview);
    });
  }

  return () => {
    previews.delete(preview);

    if (previews.size === 0 && loop) {
      cancelAnimationFrame(loop);
      loop = 0;
    }
  };
}
