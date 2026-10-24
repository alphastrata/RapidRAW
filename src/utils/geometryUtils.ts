export const IDENTITY_3X3 = [1, 0, 0, 0, 1, 0, 0, 0, 1];

export function multiply3x3(a: number[], b: number[]): number[] {
  if (!a || !b) return IDENTITY_3X3;
  const out = [0, 0, 0, 0, 0, 0, 0, 0, 0];
  for (let i = 0; i < 3; i++) {
    for (let j = 0; j < 3; j++) {
      out[i * 3 + j] = a[i * 3 + 0] * b[0 * 3 + j] + a[i * 3 + 1] * b[1 * 3 + j] + a[i * 3 + 2] * b[2 * 3 + j];
    }
  }
  return out;
}

export function invert3x3(h: number[]): number[] {
  if (!h) return IDENTITY_3X3;
  const a = h[0],
    b = h[1],
    c = h[2],
    d = h[3],
    e = h[4],
    f = h[5],
    g = h[6],
    hh = h[7],
    i = h[8];
  const A = e * i - f * hh,
    B = f * g - d * i,
    C = d * hh - e * g;
  const D = c * hh - b * i,
    E = a * i - c * g,
    F = b * g - a * hh;
  const G = b * f - c * e,
    H = c * d - a * f,
    I = a * e - b * d;
  const det = a * A + b * B + c * C;
  if (Math.abs(det) < 1e-15) return IDENTITY_3X3;
  const inv = 1.0 / det;
  return [A * inv, D * inv, G * inv, B * inv, E * inv, H * inv, C * inv, F * inv, I * inv];
}

export function project3x3(h: number[], x: number, y: number): { x: number; y: number } {
  if (!h) return { x, y };
  const W = h[6] * x + h[7] * y + h[8];
  if (Math.abs(W) < 1e-12) return { x, y };
  return {
    x: (h[0] * x + h[1] * y + h[2]) / W,
    y: (h[3] * x + h[4] * y + h[5]) / W,
  };
}

export function orientPoint(x: number, y: number, w: number, h: number, steps: number) {
  const s = ((steps % 4) + 4) % 4;
  if (s === 0) return { x, y };
  if (s === 1) return { x: h - y, y: x };
  if (s === 2) return { x: w - x, y: h - y };
  return { x: y, y: w - x };
}

export function unorientPoint(x: number, y: number, w: number, h: number, steps: number) {
  const s = ((steps % 4) + 4) % 4;
  const inv = (4 - s) % 4;
  return orientPoint(x, y, w, h, inv);
}
