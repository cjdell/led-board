/// Preview renderer: draws each `AnimationEnum` kind into a 60×40 canvas,
/// mirroring the Rust firmware renderers closely enough that the UI's cards
/// and the panel show the same thing. One shared rAF loop drives every
/// registered preview at its own frame budget (hero 24 fps, library cards
/// ~8 fps), so 28 live cards cost a few hundred setPixel calls per frame.
import { Animation } from "@lib";

export const PREVIEW_WIDTH = 60;
export const PREVIEW_HEIGHT = 40;

type Params = string | number | boolean | unknown[] | Record<string, unknown> | null | undefined;

interface Preview {
  canvas: HTMLCanvasElement;
  off: HTMLCanvasElement;
  offCtx: CanvasRenderingContext2D;
  image: ImageData;
  animation: Animation;
  kind: string;
  params: Params;
  fps: number;
  last: number;
  start: number;
  overlay?: (ctx: CanvasRenderingContext2D) => void;
}

let loop = 0;
const previews = new Set<Preview>();

// ── helpers ────────────────────────────────────────────────────────────────

function set(x: number, y: number, r: number, g: number, b: number, p: Preview) {
  if (x < 0 || x >= PREVIEW_WIDTH || y < 0 || y >= PREVIEW_HEIGHT) return;

  const i = ((y | 0) * PREVIEW_WIDTH + (x | 0)) * 4;
  const d = p.image.data;

  d[i] = r < 0 ? 0 : r > 255 ? 255 : r;
  d[i + 1] = g < 0 ? 0 : g > 255 ? 255 : g;
  d[i + 2] = b < 0 ? 0 : b > 255 ? 255 : b;
  d[i + 3] = 255;
}

function fill(r: number, g: number, b: number, p: Preview) {
  for (let y = 0; y < PREVIEW_HEIGHT; y++) {
    for (let x = 0; x < PREVIEW_WIDTH; x++) {
      set(x, y, r, g, b, p);
    }
  }
}

function circle(xc: number, yc: number, r: number, col: [number, number, number], p: Preview) {
  for (let y = yc - r; y <= yc + r; y++) {
    for (let x = xc - r; x <= xc + r; x++) {
      const dx = x - xc, dy = y - yc;

      if (dx * dx + dy * dy <= r * r) set(x, y, col[0], col[1], col[2], p);
    }
  }
}

function line(x0: number, y0: number, x1: number, y1: number, col: [number, number, number], p: Preview) {
  x0 |= 0;
  y0 |= 0;
  x1 |= 0;
  y1 |= 0;
  const dx = Math.abs(x1 - x0), dy = Math.abs(y1 - y0);
  const sx = x0 < x1 ? 1 : -1, sy = y0 < y1 ? 1 : -1;
  let err = dx - dy;

  for (;;) {
    set(x0, y0, col[0], col[1], col[2], p);
    if (x0 === x1 && y0 === y1) break;

    const e2 = 2 * err;
    if (e2 > -dy) {
      err -= dy;
      x0 += sx;
    }
    if (e2 < dx) {
      err += dx;
      y0 += sy;
    }
  }
}

function rect(x: number, y: number, w: number, h: number, col: [number, number, number], p: Preview) {
  for (let yy = y; yy < y + h; yy++) {
    for (let xx = x; xx < x + w; xx++) {
      set(xx, yy, col[0], col[1], col[2], p);
    }
  }
}

function hsv(h: number, s: number, v: number): [number, number, number] {
  h = ((h % 1) + 1) % 1;

  const i = Math.floor(h * 6), f = h * 6 - i;
  const pp = v * (1 - s), q = v * (1 - f * s), t = v * (1 - (1 - f) * s);

  switch (i % 6) {
    case 0:
      return [v * 255, t * 255, pp * 255];
    case 1:
      return [q * 255, v * 255, pp * 255];
    case 2:
      return [pp * 255, v * 255, t * 255];
    case 3:
      return [pp * 255, q * 255, v * 255];
    case 4:
      return [t * 255, pp * 255, v * 255];
    default:
      return [v * 255, pp * 255, q * 255];
  }
}

const fnv = (i: number, salt: number) => (Math.imul(i, 1677761993) ^ Math.imul(salt, 2246822519)) >>> 0;
const sinp = (a: number) => Math.sin(a); // JS Math.sin handles any argument

function strings(params: Params): string[] {
  if (Array.isArray(params)) return params.map(String);
  if (typeof params === "string") return [params];

  return [];
}

// ── painters ───────────────────────────────────────────────────────────────

type Painter = (t: number, params: Params, p: Preview) => void;

const painters: Record<string, Painter> = {
  Blank: (_t, _prm, p) => fill(0, 0, 16, p),

  Aurora: (t, _prm, p) => {
    for (let i = 0; i < 30; i++) {
      const h = fnv(i, 7);

      set(h % 60, (h >> 8) % 22, 20, 20, Math.min(255, 50 + (h % 40)), p);
    }

    for (let layer = 0; layer < 2; layer++) {
      const speed = 0.6 + layer * 0.35;
      const hueTop = layer === 0 ? 0.34 : 0.42;

      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const wave = sinp(x * 0.17 + t * speed) + 0.6 * sinp(x * 0.06 - t * speed * 0.8);
        const top = 6 + wave * 5 + layer * 4;
        const thick = 15 + 5 * sinp(x * 0.13 + t * (1 + speed));
        const shimmer = 0.55 + 0.45 * sinp(x * 0.55 + t * (2 + speed));

        for (let y = Math.floor(top); y < top + thick; y++) {
          if (y < 0 || y >= PREVIEW_HEIGHT || thick <= 1) continue;

          const fy = (y - top) / thick;
          const v = (1 - fy) * shimmer;

          if (v > 0.02) set(x, y, ...hsv(hueTop + fy * 0.4, 0.95, v), p);
        }
      }
    }
  },

  Starfield: (t, _prm, p) => {
    fill(0, 0, 8, p);

    for (let i = 0; i < 70; i++) {
      const h = fnv(i, 13);
      const tw = 0.25 + 0.75 * (sinp(t * (0.6 + ((h >> 4) % 10) * 0.15) + h * 0.05) * 0.5 + 0.5);
      const b = (tw * tw * 255) | 0;

      set(h % 60, (h >> 8) % 40, b * 0.85, b * 0.95, b, p);
    }

    const phase = t % 5.5;

    if (phase < 1) {
      for (let k = 0; k < 6; k++) {
        const f = 200 - k * 32;
        set(-4 + phase * 68 - k * 1.6, 3 + phase * 13 - k * 0.3, f, f, 255, p);
      }
    }
  },

  Snowfall: (t, _prm, p) => {
    fill(0, 0, 6, p);

    for (let i = 0; i < 60; i++) {
      const h = fnv(i, 29);
      const depth = ((h >> 4) % 10) / 9;
      const yf = ((h % PREVIEW_HEIGHT) + t * (6 + depth * 14)) % (PREVIEW_HEIGHT + 2);
      const xf = ((h >> 16) / 65536) * PREVIEW_WIDTH + t * (2 + depth * 4) + sinp(t * (0.4 + ((h >> 8) % 12) * 0.12) + i) * (3 + depth * 7);

      set(((xf % 60) + 60) % 60, yf, ...hsv(0.6, 0.15, (150 + depth * 105) / 255), p);
    }
  },

  FallingLeaves: (t, _prm, p) => {
    fill(2, 2, 4, p);

    const palette: [number, number, number][] = [[224, 96, 24], [190, 44, 32], [236, 178, 42], [150, 88, 30], [246, 214, 96]];

    for (let i = 0; i < 18; i++) {
      const h = fnv(i, 41);
      const col = palette[h % 5];
      const depth = ((h >> 3) % 10) / 9;
      const yf = (((h >> 2) % PREVIEW_HEIGHT) + t * (5 + depth * 9)) % (PREVIEW_HEIGHT + 4);
      const xf = ((h >> 16) / 65536) * 80 + sinp(t * (0.7 + ((h >> 11) % 8) * 0.1) + i) * (5 + ((h >> 7) % 6)) + t * 1.5 - 10;
      const turn = sinp(t * (1.2 + ((h >> 15) % 10) * 0.25) + i);
      const size = 1 + Math.min(2, Math.floor(Math.abs(turn) * 1.8));
      const dim = 0.5 + 0.5 * Math.abs(turn);

      circle(((xf % 80) + 80) % 80 - 10, yf - 2, size, [col[0] * dim, col[1] * dim, col[2] * dim], p);
    }
  },

  Fireflies: (t, _prm, p) => {
    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      const f = y / PREVIEW_HEIGHT;

      for (let x = 0; x < PREVIEW_WIDTH; x++) set(x, y, 4 * f, 14 * f, 8 * f, p);
    }

    circle(50, 5, 3, [150, 150, 130], p);

    for (let tx = 0; tx < 60; tx += 9) {
      const h = 6 + ((tx * 7) % 5);

      for (let y = 0; y < h; y++) rect(tx, 40 - h + y, 7 - ((y * 7) / h) | 0, 1, [3, 9, 5], p);
    }

    for (let i = 0; i < 14; i++) {
      const h = fnv(i, 57);
      const x = 8 + (h % 44) + sinp(t * (0.3 + ((h >> 3) % 10) * 0.05) + i) * 7;
      const y = 6 + ((h >> 6) % 22) + sinp(t * (0.2 + ((h >> 9) % 10) * 0.05) + i * 0.7) * 5;
      let glow = sinp(t * (0.6 + ((h >> 13) % 12) * 0.15) + i * 1.7) * 0.5 + 0.5;

      glow = glow ** 4;

      if (glow > 0.04) circle(x, y, 2, [glow * 115, glow * 255, glow * 90], p);
    }
  },

  VinylSpin: (t, _prm, p) => {
    fill(6, 6, 8, p);

    const rot = t * 3;

    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const dx = x - 30, dy = y - 20;
        const dist = Math.hypot(dx, dy);

        if (dist > 18) continue;

        const angle = Math.atan2(dy, dx);
        let v = 22 + 10 * sinp(dist * 2.5) + (0.5 + 0.5 * Math.cos(angle - rot)) ** 4 * 38;

        if (dist > 16.5) v *= (18 - dist) * 2;

        let col: [number, number, number] = dist < 6
          ? dist < 0.9 ? [0, 0, 0] : [150 + 60 * Math.max(0, 0.5 + 0.5 * Math.cos(angle - rot)), 26, 34]
          : [v * 0.75, v * 0.75, v * 0.85];

        if (dist > 2.5 && dist < 4.5 && ((angle - rot) % (Math.PI * 2) + Math.PI * 2) % (Math.PI * 2) < 0.45) col = [230, 230, 200];

        set(x, y, ...col, p);
      }
    }
  },

  CanalBoats: (t, _prm, p) => {
    const waterTop = 20;

    for (let y = 0; y < waterTop; y++) {
      const f = y / waterTop;

      for (let x = 0; x < PREVIEW_WIDTH; x++) set(x, y, 10 + 26 * f, 10 + 22 * f, 30 + 28 * f, p);
    }

    for (const [sx, sy, sv] of [[8, 3, 140], [17, 6, 95], [44, 2, 110], [52, 8, 80]] as const) set(sx, sy, sv, sv, 255, p);

    circle(49, 18, 2, [160, 110, 50], p);
    rect(41, 9, 9, 11, [8, 8, 12], p);

    for (let y = waterTop; y < PREVIEW_HEIGHT; y++) {
      const f = (y - waterTop) / (PREVIEW_HEIGHT - waterTop);

      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const ripple = 0.5 + 0.5 * sinp(x * 0.35 + t * 1.4 + y * (0.8 + 0.4 * f));

        set(x, y, 8 + 8 * ripple, 22 + 10 * f + 8 * ripple, 46 + 18 * f + 16 * ripple, p);
      }
    }

    const cycle = 18;
    const bx = Math.floor(-16 + ((t % cycle) / cycle) * (PREVIEW_WIDTH + 32));

    rect(bx, 17, 14, 2, [58, 24, 20], p);
    rect(bx, 18, 14, 1, [200, 60, 40], p);
    rect(bx + 10, 14, 1, 3, [30, 30, 34], p);
    rect(bx + 1, 14, 4, 3, [60, 40, 90], p);
    set(bx + 2, 15, 255, 200, 90, p);
    rect(bx + 1, 20, 12, 3, [12, 10, 26], p);
  },

  Rainbows: (t, _prm, p) => {
    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const dx = x - 30, dy = y - 20;
        const dist = Math.hypot(dx, dy);
        const angle = Math.atan2(dx, dy) + t * 1.5;
        const hue = (angle * 2 + dist * 0.05) / (Math.PI * 2) + t * 0.5;

        set(x, y, ...hsv(hue, 1, 1), p);
      }
    }
  },

  RainbowWheel: (t, _prm, p) => {
    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const angle = t * 0.001 + Math.atan2(y - 20, x - 30) / (Math.PI * 2);

        set(x, y, ...hsv(angle % 1, 1, 1), p);
      }
    }
  },

  SynthWave: (t, _prm, p) => {
    fill(4, 2, 10, p);

    const time = t * 50;

    for (let l = 0; l < 8; l++) {
      const y = l * 5;
      const freq = 0.4 + l * 0.05;
      const amp = 2 + l * 0.3;
      const phase = (time * freq) % (Math.PI * 2);

      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const py = y + Math.floor(Math.sin(x * 0.1 * freq + phase) * amp + amp);
        const hue = ((x + l + time / 20) % 256) / 256;

        set(x, py, ...hsv(hue, 0.8, 0.9), p);
      }
    }
  },

  Fireworks: (t, _prm, p) => {
    fill(0, 0, 5, p);

    const burst = Math.floor(t / 1.8);
    const age = t - burst * 1.8;

    for (let b = 0; b < 2; b++) {
      const h = fnv(burst + b * 7919, 17);
      const cx = 12 + (h % 36), cy = 8 + ((h >> 6) % 22);
      const r = age * 22;
      const rays = 12;

      for (let k = 0; k < rays; k++) {
        const a = (k / rays) * Math.PI * 2 + burst;
        const fade = Math.max(0, 1 - age / 1.6);
        const hue = ((h % 256) / 256) + k / rays;

        circle(cx + Math.cos(a) * r, cy + Math.sin(a) * r, 1, hsv(hue, 0.7, fade), p);
      }
    }
  },

  Circles: (t, _prm, p) => {
    fill(0, 0, 0, p);

    for (let i = 0; i < 10; i++) {
      const x = (t * 3000 >> (4 + i)) + i * 13;
      const y = (t * 3000 >> (3 + i)) + i * 7;

      circle((x % 60 + 60) % 60, (y % 40 + 40) % 40, 3 + (i % 4), hsv(i / 10 + t * 0.1, 0.9, 0.9), p);
    }
  },

  TestPattern: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const pixel = Math.floor(t * 60) % PREVIEW_WIDTH;

    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      set(pixel, y, 255, 255, 255, p);
      set(0, y, 255, 255, 255, p);
      set(PREVIEW_WIDTH - 1, y, 255, 255, 255, p);
    }
  },

  SpinningCube: (t, _prm, p) => wireframe(t, false, p),
  Ship: (t, _prm, p) => wireframe(t * 0.8, true, p),

  Columns: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const pulse = 0.5 + 0.5 * sinp(t * 1.2);

    for (let y = 0; y < PREVIEW_HEIGHT; y++) {
      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        if (x % 8 === 0) set(x, y, 15 * pulse + 5, 15 * pulse + 5, 15 * pulse + 5, p);
      }
    }
  },

  Intensity: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const beat = (0.5 + 0.5 * sinp(t * 2.4)) * 255;

    for (let x = 0; x < PREVIEW_WIDTH; x++) set(x, 0, beat, beat, beat, p);
  },

  BarChart: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const cols: [number, number, number][] = [[255, 255, 255], [255, 40, 40], [40, 255, 80], [40, 120, 255]];

    for (let b = 0; b < 4; b++) {
      const v = Math.floor((0.5 + 0.5 * sinp(t * (1.3 + b * 0.7) + b * 1.4)) * 38);

      rect(b * 15, 40 - v, 13, v, cols[b], p);
    }
  },

  LineChart: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const cols: [number, number, number][] = [[255, 255, 255], [255, 40, 40], [40, 255, 80], [40, 120, 255]];

    for (let b = 0; b < 4; b++) {
      for (let x = 0; x < 15; x++) {
        const h = Math.floor((0.5 + 0.5 * sinp(t * (1 + b * 0.4) + x * 0.4 + b)) * 9);

        set(b * 15 + x, 10 - h, ...cols[b], p);
      }
    }
  },

  EchoOfTheSoul: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const beat = 0.5 + 0.5 * sinp(t * 2.2);
    const hue = (t * 0.055) % 1;

    for (let i = 2; i >= 0; i--) {
      const r = 8 + beat * 6 + i * 5 + 2 * sinp(t + i);

      circle(30, 20, r, hsv(hue + i * 0.1, 0.9, 0.25 + 0.3 * beat), p);
    }

    circle(30, 20, 4 + beat * 6, hsv(hue, 0.5, 1), p);
  },

  HarmonicSpiral: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const cols: [number, number, number][] = [[255, 40, 120], [100, 255, 80], [60, 180, 255]];

    for (let band = 0; band < 3; band++) {
      const freq = 0.4 + 0.3 * (0.5 + 0.5 * sinp(t * (1.1 + band * 0.5) + band));
      const turns = 1.5 + freq * 3;
      const density = 20 + Math.floor(freq * 25);

      for (let i = 0; i < density; i++) {
        const angle = t * 2 + (i / density) * Math.PI * 2 * turns;
        const r = (12 + freq * 20) * (1 + Math.sin(i * 0.3 + t * 1.5) * 0.3);
        const bright = freq * (0.5 + 0.5 * Math.sin(t * 4 + i * 0.1));
        const col = cols[band].map((c) => c * bright) as [number, number, number];

        set(30 + r * Math.cos(angle), 20 + r * Math.sin(angle), ...col, p);
      }
    }
  },

  OctopusOfSound: (t, _prm, p) => {
    fill(0, 0, 6, p);

    for (let tt = 0; tt < 6; tt++) {
      const base = (tt * 10 + t * 7.5) % 60;
      const length = 12 + Math.floor(15 * (0.5 + 0.5 * sinp(t + tt)));
      const curl = sinp(t * 1.5 + tt * 0.8) * 0.5;

      for (let i = 0; i < length; i++) {
        const y = 39 - i;
        const x = base + curl * (i / length) * 10;
        const v = (1 - i / length) * 200;

        set(x, y, v * 0.4, v, v, p);
      }

      circle(base + curl * 10, 39 - length, 2, [255, 255, 255], p);
    }
  },

  MagneticField: (t, _prm, p) => {
    for (let y = 0; y < PREVIEW_HEIGHT; y += 1) {
      for (let x = 0; x < PREVIEW_WIDTH; x += 1) {
        const p1 = 15 + ((t * 6) % 30), p2 = 45 - ((t * 6) % 30);
        const d1 = Math.hypot(x - p1, y - 20), d2 = Math.hypot(x - p2, y - 20);

        if (d1 < 1 || d2 < 1) continue;

        const fx = (x - p1) / d1 + (x - p2) / d2, fy = (y - 20) / d1 + (y - 20) / d2;
        const mag = Math.hypot(fx, fy);
        const ang = Math.atan2(fy, fx) + t * 0.8;

        set(x, y, ...hsv((ang / (Math.PI * 2)) + t * 0.1, 0.9, Math.min(1, mag * 0.5)), p);
      }
    }
  },

  SilentDancer: (t, _prm, p) => {
    fill(0, 0, 0, p);

    const hue = (t * 0.17) % 1;

    for (let i = 7; i >= 0; i--) {
      const age = i;
      const x = 30 + Math.sin(t * 1.5 - age * 0.3) * 10 + Math.sin(t * 0.7 - age * 0.2) * 8;
      const y = 20 + Math.cos(t * 1.8 - age * 0.4) * 8;
      const alpha = 1 - age / 8;

      circle(x, y, i === 0 ? 2 + Math.round(Math.sin(t * 3) + 1) : 1, hsv(hue, 0.8, alpha), p);
    }
  },

  CenteredTitle: (_t, prm, p) => titleCard(strings(prm), [255, 200, 80], p),
  StatusCard: (_t, prm, p) => titleCard(strings(prm), [255, 60, 40], p),

  TwoLineMessage: (_t, prm, p) => {
    fill(0, 0, 0, p);

    const lines = strings(prm);

    drawTextOverlay([
      { text: lines[0] ?? "", x: 2, y: 4, col: [255, 255, 255], size: 12 },
      { text: lines[1] ?? "", x: 2, y: 22, col: [255, 255, 255], size: 12 },
    ], p);
  },

  ScrollingMessage: (t, prm, p) => {
    fill(0, 0, 0, p);

    const msg = Array.isArray(prm) ? String(prm[0] ?? "") : String(prm ?? "");
    const x = 60 - ((t * 50) % (msg.length * 7 + 60));

    drawTextOverlay([{ text: msg, x, y: 14 + Math.sin(t * 2) * 4, col: [255, 255, 255], size: 12 }], p);
  },
};

function titleCard(lines: string[], col: [number, number, number], p: Preview) {
  fill(0, 0, 0, p);

  drawTextOverlay(
    lines.slice(0, 2).map((line, i) => ({
      text: line,
      x: Math.max(0, (PREVIEW_WIDTH - line.length * 7) / 2),
      y: i === 0 ? 8 : 22,
      col,
      size: 12,
    })),
    p,
  );
}

function drawTextOverlay(lines: { text: string; x: number; y: number; col: [number, number, number]; size: number }[], p: Preview) {
  p.overlay = (ctx) => {
    ctx.font = `${lines[0]?.size ?? 10}px ui-monospace, Menlo, monospace`;
    ctx.textBaseline = "top";

    for (const line of lines) {
      ctx.fillStyle = `rgb(${line.col.map((c) => Math.round(c)).join(",")})`;
      ctx.fillText(line.text, line.x, line.y);
    }
  };
}

function wireframe(t: number, ship: boolean, p: Preview) {
  fill(0, 0, 0, p);

  const verts: [number, number, number][] = ship
    ? [
      [0, 0, -1.6],
      [0.2, 0, -1.2],
      [-0.2, 0, -1.2],
      [0.35, 0.1, -0.2],
      [-0.35, 0.1, -0.2],
      [0.3, 0.15, 0.8],
      [-0.3, 0.15, 0.8],
      [1.4, 0.05, 0.5],
      [-1.4, 0.05, 0.5],
      [0, 0.35, 0.9],
      [0, -0.1, 0.8],
      [0.35, 0.3, 1.2],
      [-0.35, 0.3, 1.2],
    ]
    : [[-1, -1, -1], [1, -1, -1], [1, 1, -1], [-1, 1, -1], [-1, -1, 1], [1, -1, 1], [1, 1, 1], [-1, 1, 1]];

  const edges: [number, number][] = ship
    ? [[0, 1], [0, 2], [1, 3], [2, 4], [3, 5], [4, 6], [3, 4], [5, 6], [3, 7], [7, 5], [4, 8], [8, 6], [0, 9], [5, 11], [6, 12], [11, 12], [
      9,
      10,
    ]]
    : [[0, 1], [1, 2], [2, 3], [3, 0], [4, 5], [5, 6], [6, 7], [7, 4], [0, 4], [1, 5], [2, 6], [3, 7]];

  const cx = Math.cos(t * 0.7), sx = Math.sin(t * 0.7);
  const cy = Math.cos(t * (ship ? 0.2 : 0.5)), sy = Math.sin(t * (ship ? 0.2 : 0.5));
  const scale = ship ? 16 : 11;

  const proj = verts.map(([x0, y0, z0]) => {
    let x = x0, y = y0 * cx - z0 * sx, z = y0 * sx + z0 * cx;
    const x2 = x * cy + z * sy;
    z = -x * sy + z * cy;
    x = x2;

    const persp = 3.5 + z;

    return [30 + (x * scale) / persp, 20 + (y * scale) / persp];
  });

  const col: [number, number, number] = [180, 220, 255];

  for (const [a, b] of edges) line(proj[a][0], proj[a][1], proj[b][0], proj[b][1], col, p);
}

const FALLBACK: Painter = (t, _prm, p) => {
  for (let y = 0; y < PREVIEW_HEIGHT; y++) {
    for (let x = 0; x < PREVIEW_WIDTH; x++) {
      set(x, y, ...hsv((x / 60 + y / 40) / 2 + t * 0.05, 0.75, 0.5 + 0.3 * sinp(x * 0.2 + t)), p);
    }
  }
};

// ── lifecycle ──────────────────────────────────────────────────────────────

function frame(now: number) {
  loop = requestAnimationFrame(frame);

  for (const preview of previews) {
    if (now - preview.last < 1000 / preview.fps) continue;

    preview.last = now;

    const t = (now - preview.start) / 1000;
    const painter = painters[preview.kind] ?? FALLBACK;

    preview.overlay = undefined;
    preview.image.data.fill(0); // like the firmware runner clearing both buffers
    painter(t, preview.params, preview);

    preview.offCtx.putImageData(preview.image, 0, 0);

    // The painter may have installed a text overlay (assignment happens
    // inside the callback — invisible to the narrowing above).
    const overlay = preview.overlay as Preview["overlay"];

    if (overlay) {
      overlay(preview.offCtx);
      preview.overlay = undefined;
    }

    const ctx = preview.canvas.getContext("2d")!;

    ctx.imageSmoothingEnabled = false;
    ctx.drawImage(preview.off, 0, 0, preview.canvas.width, preview.canvas.height);
  }
}

export function startPreview(canvas: HTMLCanvasElement, animation: Animation, fps = 24): () => void {
  const kind = typeof animation === "string" ? animation : Object.keys(animation)[0] ?? "Blank";
  const params = typeof animation === "string" ? null : (Object.values(animation)[0] as Params);

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
    animation,
    kind,
    params,
    fps,
    last: 0,
    start: performance.now(),
  };

  previews.add(preview);

  if (!loop) loop = requestAnimationFrame(frame);

  return () => {
    previews.delete(preview);

    if (previews.size === 0 && loop) {
      cancelAnimationFrame(loop);
      loop = 0;
    }
  };
}
