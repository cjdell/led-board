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

function ellipse(xc: number, yc: number, rx: number, ry: number, col: [number, number, number], p: Preview) {
  for (let y = Math.floor(yc - ry); y <= Math.ceil(yc + ry); y++) {
    const dy = (y - yc) / ry;
    const inner = 1 - dy * dy;

    if (inner <= 0) continue;

    const half = Math.sqrt(inner) * rx;

    for (let x = Math.ceil(xc - half); x <= Math.floor(xc + half); x++) set(x, y, col[0], col[1], col[2], p);
  }
}

function starDots(t: number, count: number, seed: number, yMax: number, p: Preview) {
  for (let i = 0; i < count; i++) {
    const h = fnv(i, seed);
    const tw = 0.25 + 0.75 * (Math.sin(t * (0.5 + ((h >> 4) % 12) * 0.17) + (h >> 12) * 0.03) * 0.5 + 0.5);
    const b = tw * tw * 220;

    set(h % 60, (h >> 8) % yMax, b * 0.85, b * 0.92, b, p);
  }
}

function snowDots(t: number, count: number, seed: number, p: Preview) {
  for (let i = 0; i < count; i++) {
    const h = fnv(i, seed);
    const depth = ((h >> 4) % 10) / 9;
    const y = (((h % 40) + t * (6 + depth * 14)) % 43) - 2;
    const x = (((h >> 16) / 65536) * 76 + t * (2 + depth * 4) + Math.sin(t * (0.4 + depth) + i) * (2 + depth * 3)) % 76 - 8;

    set(x, y, 180 + depth * 70, 200 + depth * 50, 255, p);
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
    const waterTop = 23;

    // Sunset sky.
    for (let y = 0; y < waterTop; y++) {
      const f = y / waterTop;

      for (let x = 0; x < PREVIEW_WIDTH; x++) set(x, y, 40 + 190 * f * f, 22 + 90 * f * f, 84 - 30 * f, p);
    }
    // Sun.
    circle(50, 20, 4, [255, 224, 160], p);
    // Mill + bridge silhouettes.
    rect(39, 8, 10, 15, [22, 16, 24], p);
    rect(43, 3, 2, 5, [22, 16, 24], p);
    rect(0, 12, 18, 11, [36, 30, 38], p);
    circle(9, 24, 5, [24, 34, 58], p);
    // Water.
    for (let y = waterTop; y < PREVIEW_HEIGHT; y++) {
      const f = (y - waterTop) / (PREVIEW_HEIGHT - waterTop);

      for (let x = 0; x < PREVIEW_WIDTH; x++) {
        const ripple = 0.5 + 0.5 * sinp(x * 0.45 + t * 1.8 + y * 1.3);

        set(x, y, 16 + 22 * f + 8 * ripple, 26 + 18 * f + 10 * ripple, 54 + 24 * f + 18 * ripple, p);
      }
    }
    // Boat.
    const cycle = 22;
    const bx = Math.floor(-22 + ((t % cycle) / cycle) * 100);

    rect(bx, 20, 18, 3, [32, 30, 38], p);
    rect(bx, 21, 18, 1, [180, 46, 40], p);
    rect(bx + 2, 15, 12, 5, [210, 196, 160], p);
    rect(bx + 3, 16, 2, 2, [255, 200, 100], p);
    rect(bx + 7, 16, 2, 2, [255, 190, 80], p);
    rect(bx + 11, 16, 2, 2, [255, 200, 100], p);
    rect(bx + 12, 12, 1, 3, [60, 50, 50], p);
  },

  // ── Festival scenes (new in the lantern-parade set) ─────────────────────
  Robin: (t, _prm, p) => {
    for (let y = 0; y < 40; y++) {
      const f = y / 40;

      for (let x = 0; x < 60; x++) set(x, y, 28 + 104 * f, 48 + 118 * f, 78 + 112 * f, p);
    }
    circle(46, 8, 4, [255, 236, 196], p);
    snowDots(t, 24, 5, p);
    // Branch.
    for (let k = 0; k < 4; k++) line(-2, 32 + k, 58, 38 + (k >> 1), [38, 26, 22], p);
    line(2, 31, 58, 37, [228, 236, 244], p);
    // Robin.
    const bob = Math.sin(t * 1.7) * 0.8;

    ellipse(27, 23 + bob, 8, 9, [58, 40, 32], p);
    ellipse(28, 25 + bob, 6, 6, [226, 82, 38], p);
    ellipse(28, 27 + bob, 4, 4, [250, 128, 52], p);
    ellipse(30, 16 + bob, 5, 5, [52, 36, 30], p);
    set(33, 15, 255, 255, 255, p);
    set(34, 15, 8, 8, 10, p);
    line(36, 15, 39, 16, [255, 176, 46], p);
    line(23, 20 + bob, 19, 32, [44, 30, 24], p);
  },

  Owl: (t, _prm, p) => {
    fill(4, 10, 24, p);
    starDots(t, 30, 33, 26, p);
    circle(48, 8, 6, [238, 234, 208], p);
    for (let x = 2; x < 60; x += 11) line(x, 34, x, 26 - ((x * 7) % 5), [8, 20, 24], p);
    rect(2, 0, 6, 40, [18, 20, 22], p);
    line(18, 31, 58, 33, [26, 22, 20], p);
    // Body and head.
    ellipse(35, 25 + Math.sin(t * 1.1) * 0.5, 8, 9, [48, 34, 24], p);
    ellipse(35, 26, 6, 7, [66, 46, 30], p);
    ellipse(33, 15, 6, 6, [52, 38, 26], p);
    circle(30, 15, 2, [255, 206, 60], p);
    circle(37, 15, 2, [255, 206, 60], p);
    circle(30, 15, 1, [10, 8, 6], p);
    circle(37, 15, 1, [10, 8, 6], p);
    line(33, 17, 34, 20, [190, 150, 70], p);
  },

  Deer: (t, _prm, p) => {
    fill(6, 14, 30, p);
    starDots(t, 30, 61, 24, p);
    circle(9, 9, 7, [238, 234, 208], p);
    for (let x = 0; x < 60; x += 9) line(x, 30, x, 20 + ((x * 5) % 7), [9, 24, 30], p);
    for (let x = 0; x < 60; x++) {
      const h = 3 + Math.round(Math.sin(x * 0.19 + 1.4) * 2);

      for (let y = 33 - h; y < 40; y++) set(x, y, 12, 29, 34, p);
    }
    const lift = Math.pow(0.5 + 0.5 * Math.sin(t * 0.42), 3);
    const hx = 38, hy = 15 - lift * 3;

    for (const [x0, x1] of [[19, 18], [24, 25], [29, 30], [33, 34]]) line(x0, 27, x1, 36, [12, 10, 10], p);
    ellipse(25, 24, 9, 4, [14, 12, 12], p);
    ellipse(21, 24, 3, 3, [14, 12, 12], p);
    line(30, 24, 34, 19, [14, 12, 12], p);
    ellipse(34, 19, 2, 4, [14, 12, 12], p);
    ellipse(hx, hy, 3, 2, [16, 13, 12], p);
    // Antlers.
    for (const [dx, dy] of [[0, -2], [1, -4], [2, -6], [3, -8], [-1, -4], [-2, -6], [0, -8], [1, -7], [4, -6]]) {
      line(38, 12, 38 + dx, 12 + dy, [74, 56, 34], p);
    }
    set(hx + 2, hy, 255, 190, 80, p);
  },

  SnowLeopard: (t, _prm, p) => {
    fill(4, 12, 30, p);
    starDots(t, 30, 41, 20, p);
    circle(12, 8, 6, [238, 234, 208], p);
    for (let x = 0; x < 60; x++) {
      const h = 7 + Math.round(Math.sin(x * 0.11 + 2) * 4 + Math.sin(x * 0.31) * 2);

      for (let y = 28 - h; y < 34; y++) set(x, y, 30, 46, 66, p);
      for (let y = 28 - h; y < 30 - (h >> 1); y++) set(x, y, 92, 116, 140, p);
    }
    // Head.
    ellipse(30, 17, 11, 9, [136, 146, 154], p);
    // Ears.
    line(20, 11, 18, 4, [136, 146, 154], p);
    line(18, 4, 26, 8, [136, 146, 154], p);
    line(40, 11, 42, 4, [136, 146, 154], p);
    line(42, 4, 34, 8, [136, 146, 154], p);
    // Rosettes.
    for (const [sx, sy] of [[23, 10], [27, 9], [33, 9], [37, 10], [21, 14], [39, 14], [22, 20], [38, 20], [24, 24], [36, 24]]) {
      set(sx, sy, 52, 58, 68, p);
      set(sx + 1, sy, 52, 58, 68, p);
    }
    // Eyes and muzzle.
    if (t % 4.6 > 0.13) {
      circle(25, 15, 2, [96, 226, 142], p);
      circle(35, 15, 2, [96, 226, 142], p);
      set(25, 15, 16, 30, 22, p);
      set(35, 15, 16, 30, 22, p);
    } else {
      line(23, 15, 27, 15, [48, 40, 44], p);
      line(33, 15, 37, 15, [48, 40, 44], p);
    }
    ellipse(30, 22, 5, 3, [186, 192, 192], p);
    set(30, 21, 104, 84, 90, p);
    line(25, 23, 18, 22, [210, 218, 226], p);
    line(35, 23, 42, 22, [210, 218, 226], p);
    // Chest.
    ellipse(30, 36, 12, 7, [136, 146, 154], p);
    ellipse(30, 36, 7, 5, [196, 202, 200], p);
  },

  FatherChristmas: (t, _prm, p) => {
    fill(4, 6, 24, p);
    starDots(t, 45, 71, 26, p);
    circle(16, 11, 9, [238, 234, 208], p);
    // Rooftops.
    for (let x = 0; x < 60; x++) {
      const h = 5 + ((x * 7 + 3) % 6);

      for (let y = 34 - h; y < 40; y++) set(x, y, 10, 10, 22, p);
    }
    for (const [x, y] of [[8, 33], [22, 35], [31, 32], [46, 34], [52, 31]]) set(x, y, 255, 172, 62, p);
    // Sleigh group.
    const cycle = 12;
    const sx = Math.floor(-40 + ((t % cycle) / cycle) * 120);
    const sy = 14 + Math.round(Math.sin(t * 1.6));

    for (const dx of [15, 29]) {
      const rx = sx + dx, ry = sy;

      ellipse(rx, ry, 4, 2, [48, 32, 24], p);
      line(rx + 2, ry, rx + 5, ry - 3, [48, 32, 24], p);
      line(rx + 5, ry - 4, rx + 4, ry - 7, [110, 82, 54], p);
      line(rx + 4, ry - 7, rx + 2, ry - 9, [110, 82, 54], p);
      line(rx - 2, ry + 1, rx - 3, ry + 5, [48, 32, 24], p);
      line(rx + 2, ry + 1, rx + 3, ry + 5, [48, 32, 24], p);
    }
    set(sx + 36, sy - 3, 255, 40, 40, p);
    line(sx - 4, sy + 5, sx + 13, sy + 5, [210, 166, 70], p);
    line(sx + 11, sy + 5, sx + 13, sy + 2, [210, 166, 70], p);
    rect(sx - 3, sy - 4, 4, 5, [150, 22, 28], p);
    line(sx, sy - 1, sx + 9, sy - 1, [230, 180, 90], p);
    ellipse(sx + 5, sy + 1, 5, 2.5, [170, 28, 32], p);
    circle(sx + 2, sy - 3, 2, [200, 32, 34], p);
    set(sx + 4, sy - 4, 240, 236, 220, p);
    circle(sx - 6, sy - 1, 2, [110, 74, 36], p);
  },

  FrostFairies: (t, _prm, p) => {
    fill(2, 7, 24, p);
    starDots(t, 40, 13, 30, p);
    circle(48, 9, 4, [210, 236, 244], p);
    line(3, 38, 10, 26, [70, 140, 160], p);
    line(57, 38, 50, 26, [70, 140, 160], p);
    const path = (tt: number) => [30 + Math.sin(tt * 0.51 + 0.4) * 18, 18 + Math.sin(tt * 0.73 + 1.7) * 9];
    // Trail.
    for (let i = 25; i >= 0; i--) {
      const [tx, ty] = path(t - i * 0.05);
      const f = 1 - i / 26;

      set(tx, ty, 120 + 110 * f, 200 + 40 * f, 255, p);
    }
    const [fx, fy] = path(t);

    circle(fx, fy, 2, [245, 252, 255], p);
    line(fx + 1, fy - 1, fx + 5, fy - 4, [140, 220, 255], p);
    line(fx + 1, fy + 1, fx + 5, fy + 4, [210, 160, 255], p);
    line(10, 35, 12, 31, [150, 220, 236], p);
    line(10, 35, 8, 31, [150, 220, 236], p);
  },

  SnowGlobe: (t, _prm, p) => {
    fill(18, 8, 30, p);
    starDots(t, 26, 27, 34, p);
    const energy = Math.max(0, 1 - ((t * 1.4) % 9) / 5);
    const cx = 30, cy = 18, r = 14;

    circle(cx, cy, r, [14, 28, 54], p);
    // Cottage.
    rect(cx - 7, 16, 9, 8, [52, 38, 38], p);
    line(cx - 9, 16, cx - 3, 10, [230, 236, 240], p);
    line(cx - 3, 10, cx + 3, 16, [230, 236, 240], p);
    rect(cx - 5, 18, 2, 2, [255, 190, 90], p);
    line(cx - 12, 25, cx + 12, 25, [210, 226, 236], p);
    // Snow.
    for (let i = 0; i < 34; i++) {
      const h = fnv(i, 111);
      const x = cx + ((h % 27) - 13) + Math.sin(t * (2.6 + energy * 7) + i) * (1.5 + energy * 3);
      const y = cy + (((h >> 8) % 24) - 12) + (energy * 10 + i) % 20 - 10;

      if ((x - cx) ** 2 + (y - cy) ** 2 < (r - 1) ** 2) set(x, y, 235, 246, 255, p);
    }
    line(cx - 10, 32, cx + 10, 32, [170, 120, 62], p);
    rect(cx - 9, 33, 19, 4, [112, 72, 40], p);
    line(cx - 7, 34, cx + 7, 34, [210, 168, 80], p);
  },

  Penguins: (t, _prm, p) => {
    fill(4, 10, 28, p);
    starDots(t, 26, 43, 20, p);
    // Aurora wash.
    for (let y = 2; y < 12; y++) {
      for (let x = 0; x < 60; x++) {
        if (Math.sin(x * 0.09 + t * 0.35) > -0.2) set(x, y, 20, 80 + y * 3, 70, p);
      }
    }
    rect(0, 25, 60, 4, [206, 226, 238], p);
    for (let y = 29; y < 40; y++) {
      for (let x = 0; x < 60; x++) {
        const ripple = 0.5 + 0.5 * Math.sin(x * 0.4 + t * 1.5 + y);

        set(x, y, 28 + ripple * 8, 70 + ripple * 10, 110 + ripple * 14, p);
      }
    }
    const peng = (x: number, y: number, s: number, w: number) => {
      ellipse(x + w, y, 3.6 * s, 5.6 * s, [12, 16, 24], p);
      ellipse(x + w + 0.4, y + 0.8 * s, 2.4 * s, 4 * s, [232, 236, 240], p);
      ellipse(x + w, y - 4.2 * s, 2.7 * s, 2.7 * s, [12, 16, 24], p);
      set(x + w + 1, y - 4, 255, 170, 40, p);
    };

    peng(14, 22, 1, Math.sin(t * 3.2) * 0.4);
    peng(24, 23, 0.8, Math.sin(t * 3.2 + 2.7) * 0.3);
    const slideX = 40 + Math.sin(t * 0.8) * 14;

    ellipse(slideX, 28, 5, 2, [12, 16, 24], p);
    ellipse(slideX + 0.5, 28, 3.4, 1, [232, 236, 240], p);
    circle(slideX + 5, 27, 2, [12, 16, 24], p);
  },

  Reindeer: (t, _prm, p) => {
    for (let y = 0; y < 40; y++) {
      const f = y / 40;

      for (let x = 0; x < 60; x++) set(x, y, 24 + 182 * f, 14 + 108 * f, 46 + 22 * f, p);
    }
    circle(46, 24, 5, [255, 218, 150], p);
    for (let x = 0; x < 60; x += 10) line(x, 34, x, 26 - ((x * 3) % 6), [26, 18, 30], p);
    for (let x = 0; x < 60; x++) {
      const h = 4 + Math.round(Math.sin(x * 0.12 + 1) * 2);

      for (let y = 33 - h; y < 40; y++) set(x, y, 198, 206, 224, p);
    }
    const phase = (t % 4.2) / 4.2;
    const leap = Math.sin(phase * Math.PI);
    const cx = -8 + phase * 78;
    const cy = 25 - leap * 9;
    const leg = Math.sin(t * 4);

    ellipse(cx, cy, 7, 3, [54, 30, 20], p);
    line(cx + 4, cy - 1, cx + 8, cy - 6, [54, 30, 20], p);
    ellipse(cx + 9, cy - 7, 3, 2, [66, 38, 22], p);
    for (const [dx, dy] of [[0, -2], [1, -4], [2, -6], [-1, -4], [3, -3], [2, -7]]) {
      line(cx + 9, cy - 9, cx + 9 + dx, cy - 9 + dy, [112, 74, 36], p);
    }
    line(cx - 5, cy + 2, cx - 9 + leap * 2, cy + 7, [58, 32, 20], p);
    line(cx - 2, cy + 2, cx - 3 - leap * 2, cy + 8, [58, 32, 20], p);
    line(cx + 3, cy + 2, cx + 8 - leap, cy + 6, [58, 32, 20], p);
    line(cx + 6, cy - 3, cx - 2, cy - 4 + leg, [200, 40, 44], p);
    set(cx + 10, cy - 7, 20, 12, 10, p);
  },

  ElectricGuitar: (t, _prm, p) => {
    fill(5, 1, 14, p);
    for (let i = 0; i < 10; i++) {
      const h = Math.floor(3 + 8 * (0.5 + 0.5 * Math.sin(t * 2 + i * 0.9)));

      rect(2 + i * 6, 40 - h, 1, h, [36, 16, 60], p);
    }
    // Body.
    circle(27, 28, 8, [150, 18, 62], p);
    circle(28, 20, 6, [158, 20, 68], p);
    rect(23, 20, 10, 8, [156, 20, 66], p);
    circle(27, 26, 2, [22, 6, 18], p);
    circle(23, 29, 2, [230, 188, 90], p);
    // Neck and strings.
    line(32, 20, 49, 5, [130, 84, 42], p);
    line(33, 20, 50, 5, [130, 84, 42], p);
    rect(47, 2, 6, 5, [28, 16, 28], p);
    line(26, 30, 49, 6, [220, 240, 255], p);
    line(28, 30, 51, 6, [180, 220, 240], p);
    // Buzzcock.
    const wx = 30 + Math.cos(t * 1.5) * 22;
    const wy = 15 + Math.sin(t * 1.5) * 8;

    for (let k = 0; k < 4; k++) set(wx - k, wy, k % 2 ? 20 : 255, k % 2 ? 16 : 200, k % 2 ? 18 : 30, p);
    line(wx + 1, wy - 1, wx + 3, wy - 3, [210, 235, 255], p);
  },

  MadameRatcliffe: (t, _prm, p) => {
    fill(30, 2, 18, p);
    for (let x = 0; x < 12; x++) for (let y = 0; y < 40; y++) set(x, y, 90, 10, 40, p);
    for (let x = 48; x < 60; x++) for (let y = 0; y < 40; y++) set(x, y, 90, 10, 40, p);
    for (let y = 33; y < 40; y++) for (let x = 12; x < 48; x++) set(x, y, 56, 14, 26, p);
    // Bust and face.
    line(14, 40, 29, 25, [38, 8, 26], p);
    line(46, 40, 29, 25, [38, 8, 26], p);
    rect(25, 19, 9, 6, [226, 214, 196], p);
    ellipse(28, 16, 4, 5, [234, 208, 180], p);
    line(32, 15, 35, 16, [234, 208, 180], p);
    set(31, 15, 30, 14, 12, p);
    set(31, 18, 190, 44, 66, p);
    // Hat.
    ellipse(28, 10, 12, 2, [46, 10, 34], p);
    rect(23, 2, 13, 8, [54, 12, 40], p);
    rect(23, 8, 13, 1, [255, 200, 88], p);
    line(34, 7, 49, 3 + Math.sin(t), [240, 200, 130], p);
    // Fan.
    for (let k = 0; k < 5; k++) line(43, 29, 43 + Math.cos(-1 + k * 0.2) * 7, 29 + Math.sin(-1 + k * 0.2) * 7, [220, 90, 130], p);
    // Hats.
    rect(5, 4, 6, 5, [16, 12, 18], p);
    ellipse(51, 10, 4, 3, [36, 28, 44], p);
    ellipse(7, 30, 4, 2, [230, 190, 90], p);
    ellipse(52, 31, 3, 2, [190, 30, 90], p);
  },

  Parrot: (t, _prm, p) => {
    fill(4, 26, 34, p);
    for (let x = 2; x < 60; x += 9) line(x, 0, x, 10 + ((x * 5) % 10), [16, 64, 38], p);
    line(0, 27, 60, 31, [88, 56, 30], p);
    // Body and wing.
    const bob = Math.sin(t * 2.2) * 0.6;
    const cx = 31, cy = 20 + bob;

    for (let k = 0; k < 3; k++) line(cx - 4, cy + 3, cx - 12 - k * 2, cy + 13 + k * 2, k === 1 ? [32, 80, 210] : [220, 34, 44], p);
    ellipse(cx, cy, 5, 8, [210, 32, 40], p);
    ellipse(cx + 1, cy + 1, 3, 6, [238, 64, 48], p);
    ellipse(cx - 3, cy - 2 - (0.5 + 0.5 * Math.sin(t * 7.5)) * 4, 3, 5, [32, 90, 220], p);
    ellipse(cx + 2, cy - 7, 4, 4, [220, 38, 44], p);
    ellipse(cx + 4, cy - 7, 2, 2, [245, 238, 220], p);
    set(cx + 3, cy - 8, 10, 10, 10, p);
    line(cx + 6, cy - 8, cx + 10, cy - 6, [28, 24, 24], p);
    line(cx + 6, cy - 6, cx + 10, cy - 4, [28, 24, 24], p);
    set(8, 18, 255, 220, 120, p);
  },

  CircusTent: (t, _prm, p) => {
    fill(6, 4, 22, p);
    starDots(t, 30, 59, 18, p);
    for (let y = 4; y < 30; y++) {
      const half = Math.floor((y - 4) * 1.4);

      for (let x = 30 - half; x <= 30 + half; x++) {
        const stripe = (Math.abs(x - 30) / 3 | 0) % 2 === 0;

        set(x, y, stripe ? 196 : 238, stripe ? 36 : 224, stripe ? 48 : 196, p);
      }
    }
    line(30, 4, 30, 0, [220, 200, 150], p);
    line(31, 0, 37, 2, [220, 50, 50], p);
    line(31, 0, 31, 4, [220, 50, 50], p);
    ellipse(30, 30, 5, 7, [255, 190, 90], p);
    ellipse(30, 30, 3, 5, [80, 20, 30], p);
    // Clown.
    const by = 33 + Math.sin(t * 2) * 0.5;

    circle(46, by, 3, [210, 60, 80], p);
    line(46, by - 3, 46, by - 10, [240, 240, 230], p);
    line(46, by - 8, 41, by - 12, [240, 240, 230], p);
    line(46, by - 8, 51, by - 12, [240, 240, 230], p);
    circle(46, by - 13, 2, [250, 220, 180], p);
    line(44, by - 15, 48, by - 15, [220, 60, 70], p);
    for (let k = 0; k < 3; k++) {
      const ph = t * 2.2 + k * 2.1;

      circle(46 + Math.sin(ph) * 7, by - 16 - Math.abs(Math.cos(ph)) * 5, 1, [80 + k * 60, 200 - k * 40, 220], p);
    }
  },

  Elephants: (t, _prm, p) => {
    fill(24, 7, 40, p);
    for (let x = 0; x < 60; x += 8) line(x, 4, x + 3, 7, [230, 200, 100], p);
    for (let x = 0; x < 60; x += 3) set(x, 34, 255, 160, 40, p);
    const elephant = (cx: number, cy: number, s: number, phase: number, body: [number, number, number], ear: [number, number, number]) => {
      for (let k = 0; k < 4; k++) {
        const x = cx + (-7 + k * 4.5) * s;
        const dx = Math.sin(phase + k * 1.5) * s;

        rect(x + dx, cy + 3 * s, 2.5 * s, 5 * s, [90, 64, 110], p);
      }
      ellipse(cx, cy, 10 * s, 6 * s, body, p);
      rect(cx - 6 * s, cy - 5 * s, 12 * s, 5 * s, [210, 50, 78], p);
      rect(cx - 6 * s, cy - 5 * s, 12 * s, 1, [255, 204, 80], p);
      ellipse(cx + 10 * s, cy - s, 5 * s, 5 * s, body, p);
      ellipse(cx + 8 * s, cy, 3.5 * s, 4.5 * s, ear, p);
      for (let k = 0; k < 7; k++) {
        const f = k / 6;

        circle(cx + (13.6 + f * 2.6) * s, cy + (2 + f * 7.5) * s, Math.max(1, (2.1 - f * 1.2) * s), body, p);
      }
      set(cx + 11 * s, cy - 2 * s, 22, 12, 22, p);
      line(cx + 13 * s, cy - 4 * s, cx + 16 * s, cy - 10 * s, [246, 82, 104], p);
    };

    elephant(13, 27, 0.62, t * 2 + 1.4, [112, 82, 138], [178, 122, 154]);
    elephant(33, 23.5, 1, t * 2.2, [156, 118, 182], [224, 152, 184]);
    for (let i = 0; i < 22; i++) {
      const h = fnv(i, 197);

      set(h % 60, (t * (2 + ((h >> 3) % 4)) + ((h >> 8) % 40)) % 40, ...hsv((i * 0.19) % 1, 0.8, 0.9), p);
    }
  },

  SpinningWheel: (t, _prm, p) => {
    for (let y = 0; y < 40; y++) {
      const f = y / 40;

      for (let x = 0; x < 60; x++) set(x, y, 28 + 30 * f, 16 + 16 * f, 18 + 6 * f, p);
    }
    rect(43, 5, 14, 14, [18, 26, 44], p);
    rect(44, 6, 12, 12, [50, 70, 108], p);
    circle(52, 8, 3, [230, 230, 210], p);
    // Wheel.
    circle(23, 20, 12, [88, 56, 30], p);
    circle(23, 20, 11, [56, 34, 20], p);
    for (let k = 0; k < 8; k++) {
      const a = t * 2.5 + (k * Math.PI) / 4;

      line(23, 20, 23 + Math.cos(a) * 11, 20 + Math.sin(a) * 11, [122, 82, 44], p);
    }
    line(23, 32, 14, 37, [70, 44, 24], p);
    line(23, 32, 32, 37, [70, 44, 24], p);
    // Fleece and basket.
    circle(8, 11, 4, [236, 224, 200], p);
    circle(9, 9, 2, [255, 246, 224], p);
    rect(2, 34, 8, 5, [110, 72, 38], p);
    // Cat.
    ellipse(47, 36, 7, 3, [44, 30, 24], p);
    circle(41, 34, 3, [50, 34, 28], p);
    line(53, 36, 57, 34 + Math.sin(t * 1.8) * 2, [44, 30, 24], p);
    // Window glow.
    for (let y = 0; y < 20; y++) for (let x = 46; x < 60; x++) if ((x + y) % 7 === 0) set(x, y, 70, 90, 130, p);
  },

  MillBuildings: (t, _prm, p) => {
    fill(6, 10, 30, p);
    starDots(t, 30, 67, 18, p);
    circle(45, 22, 3, [255, 210, 140], p);
    for (const [bx, bw, bh] of [[4, 14, 12], [20, 16, 16], [39, 12, 10]]) {
      rect(bx, 34 - bh, bw, bh, [20, 18, 30], p);
      for (let k = 0; k < bw / 4; k++) {
        line(bx + k * 4, 34 - bh, bx + k * 4 + 4, 34 - bh - 3, [38, 34, 52], p);
        line(bx + k * 4 + 4, 34 - bh - 3, bx + k * 4 + 4, 34 - bh, [38, 34, 52], p);
      }
      for (let wy = 34 - bh + 3; wy < 33; wy += 3) {
        for (let wx = bx + 2; wx < bx + bw - 1; wx += 3) {
          if (Math.sin((wx + wy) * 0.7 + t) > -0.6) set(wx, wy, 255, 180, 80, p);
        }
      }
    }
    rect(10, 2, 2, 32, [24, 20, 30], p);
    rect(44, -1, 2, 35, [24, 20, 30], p);
    // Gear.
    circle(30, 29, 8, [80, 70, 80], p);
    circle(30, 29, 7, [30, 26, 34], p);
    for (let k = 0; k < 10; k++) {
      const a = t * 0.8 + (k * Math.PI) / 5;

      line(30, 29, 30 + Math.cos(a) * 8, 29 + Math.sin(a) * 8, [100, 84, 76], p);
    }
    // Canal.
    for (let y = 35; y < 40; y++) for (let x = 0; x < 60; x++) set(x, y, 12 + (y - 35), 26 + (y - 35) * 2, 46 + (y - 35) * 3, p);
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
