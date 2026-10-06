import { describe, expect, it } from 'vitest';
import {
  DUCK_BLEND_MODE,
  interpolateDuckSprites,
} from './duck-tracking';

function weightAt(
  weights: ReturnType<typeof interpolateDuckSprites>,
  yaw: number,
  pitch: number,
) {
  return weights.find(
    (weight) => weight.yaw === yaw && weight.pitch === pitch,
  )?.opacity;
}

describe('interpolateDuckSprites', () => {
  it('shows exactly one nearest sprite by default', () => {
    expect(DUCK_BLEND_MODE).toBe('nearest');
    const weights = interpolateDuckSprites(2, 1);
    expect(
      weights.filter((weight) => weight.opacity === 1),
    ).toHaveLength(1);
    expect(weights.filter((weight) => weight.opacity > 0)).toHaveLength(
      1,
    );
    expect(
      weights.reduce((sum, weight) => sum + weight.opacity, 0),
    ).toBeCloseTo(1);
  });

  it('selects each exact corner and center sprite', () => {
    for (const [yaw, pitch] of [
      [-8, -5],
      [0, 0],
      [8, 5],
    ] as const) {
      const weights = interpolateDuckSprites(yaw, pitch);
      expect(weightAt(weights, yaw, pitch)).toBe(1);
      expect(
        weights.reduce((sum, weight) => sum + weight.opacity, 0),
      ).toBeCloseTo(1);
    }
  });

  it('gives the diagonal corner sprites a wider selection region', () => {
    const topRight = interpolateDuckSprites(3.2, -2.1);
    expect(weightAt(topRight, 8, -5)).toBe(1);

    const bottomLeft = interpolateDuckSprites(-3.2, 2.1);
    expect(weightAt(bottomLeft, -8, 5)).toBe(1);
  });

  it('bilinearly blends the four surrounding sprites at midpoints', () => {
    const weights = interpolateDuckSprites(-4, -2.5, 'bilinear');
    expect(weightAt(weights, -8, -5)).toBeCloseTo(0.25);
    expect(weightAt(weights, 0, -5)).toBeCloseTo(0.25);
    expect(weightAt(weights, -8, 0)).toBeCloseTo(0.25);
    expect(weightAt(weights, 0, 0)).toBeCloseTo(0.25);
    expect(
      weights.reduce((sum, weight) => sum + weight.opacity, 0),
    ).toBeCloseTo(1);
  });

  it('clamps out-of-range angles to the sprite grid', () => {
    const weights = interpolateDuckSprites(80, -50);
    expect(weightAt(weights, 8, -5)).toBe(1);
    expect(
      weights.reduce((sum, weight) => sum + weight.opacity, 0),
    ).toBeCloseTo(1);
  });

  it('renormalizes the two strongest sprites when requested', () => {
    const weights = interpolateDuckSprites(2, 1, 'top-two');
    expect(weights.filter((weight) => weight.opacity > 0)).toHaveLength(
      2,
    );
    expect(
      weights.reduce((sum, weight) => sum + weight.opacity, 0),
    ).toBeCloseTo(1);
  });
});
