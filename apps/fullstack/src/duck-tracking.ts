export const YAWS = [-8, 0, 8] as const;
export const PITCHES = [-5, 0, 5] as const;
export const DUCK_BLEND_MODE: 'bilinear' | 'top-two' | 'nearest' =
  'nearest';
const NEAREST_ENDPOINT_THRESHOLD = 0.62;

export interface DuckSpriteWeight {
  yaw: number;
  pitch: number;
  opacity: number;
}

export function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

export function interpolateDuckSprites(
  yaw: number,
  pitch: number,
  mode: 'bilinear' | 'top-two' | 'nearest' = DUCK_BLEND_MODE,
): DuckSpriteWeight[] {
  const gx = (clamp(yaw, YAWS[0], YAWS[2]) + 8) / 8;
  const gy = (clamp(pitch, PITCHES[0], PITCHES[2]) + 5) / 5;
  const x0 = Math.floor(gx);
  const x1 = Math.min(x0 + 1, 2);
  const y0 = Math.floor(gy);
  const y1 = Math.min(y0 + 1, 2);
  const tx = gx - x0;
  const ty = gy - y0;
  const corners: DuckSpriteWeight[] = [
    {
      yaw: YAWS[x0]!,
      pitch: PITCHES[y0]!,
      opacity: (1 - tx) * (1 - ty),
    },
    { yaw: YAWS[x1]!, pitch: PITCHES[y0]!, opacity: tx * (1 - ty) },
    { yaw: YAWS[x0]!, pitch: PITCHES[y1]!, opacity: (1 - tx) * ty },
    { yaw: YAWS[x1]!, pitch: PITCHES[y1]!, opacity: tx * ty },
  ];
  const combined = new Map<string, DuckSpriteWeight>();
  for (const corner of corners) {
    const key = `${corner.yaw}:${corner.pitch}`;
    const existing = combined.get(key);
    if (existing) existing.opacity += corner.opacity;
    else combined.set(key, { ...corner });
  }
  const weights = [...combined.values()];

  if (mode === 'bilinear') return weights;
  if (mode === 'nearest') {
    const nearestIndex = (coordinate: number) =>
      coordinate < 1
        ? coordinate <= NEAREST_ENDPOINT_THRESHOLD
          ? 0
          : 1
        : coordinate >= 2 - NEAREST_ENDPOINT_THRESHOLD
          ? 2
          : 1;
    const nearestYaw = YAWS[nearestIndex(gx)]!;
    const nearestPitch = PITCHES[nearestIndex(gy)]!;
    return weights.map((weight) => ({
      ...weight,
      opacity:
        weight.yaw === nearestYaw && weight.pitch === nearestPitch
          ? 1
          : 0,
    }));
  }
  const top = [...weights]
    .sort((a, b) => b.opacity - a.opacity)
    .slice(0, 2);
  const total = top.reduce((sum, weight) => sum + weight.opacity, 0);
  const retained = new Set(top);
  return weights.map((weight) => ({
    ...weight,
    opacity:
      retained.has(weight) && total > 0 ? weight.opacity / total : 0,
  }));
}

interface DuckNode {
  element: HTMLElement;
  sprites: Map<string, HTMLImageElement>;
  opacity: Map<string, number>;
  bounds: DOMRect;
  resizeObserver: ResizeObserver;
}

const EPSILON = 0.015;
const DAMPING_MS = 120;

export function installDuckTracking(root: HTMLElement = document.body) {
  const reducedMotion = window.matchMedia(
    '(prefers-reduced-motion: reduce)',
  );
  const coarsePointer = window.matchMedia('(pointer: coarse)');
  let duck: DuckNode | null = null;
  let targetYaw = 0;
  let targetPitch = 0;
  let currentYaw = 0;
  let currentPitch = 0;
  let frame: number | null = null;
  let previousFrameTime: number | null = null;

  const isTrackingEnabled = () =>
    !reducedMotion.matches && !coarsePointer.matches;

  function writeSprites(yaw: number, pitch: number) {
    if (!duck) return;
    const spriteWeights = interpolateDuckSprites(yaw, pitch);
    const activeSprite = spriteWeights.find(
      (weight) => weight.opacity === 1,
    );
    const weights = new Map(
      spriteWeights.map((weight) => [
        `${weight.yaw}:${weight.pitch}`,
        weight.opacity,
      ]),
    );
    for (const [key, image] of duck.sprites) {
      const next = weights.get(key) ?? 0;
      const previous = duck.opacity.get(key);
      if (previous !== undefined && Math.abs(previous - next) < 0.002)
        continue;
      image.style.opacity = String(next);
      duck.opacity.set(key, next);
    }
    if (activeSprite) {
      const activeKey = `${activeSprite.yaw}:${activeSprite.pitch}`;
      const image = duck.sprites.get(activeKey);
      if (image) {
        const yawRoll = clamp(yaw - activeSprite.yaw, -4, 4);
        const pitchRoll = clamp(pitch - activeSprite.pitch, -2.5, 2.5);
        const transform = `rotateY(${yawRoll.toFixed(2)}deg) rotateX(${pitchRoll.toFixed(2)}deg)`;
        if (image.style.transform !== transform)
          image.style.transform = transform;
      }
    }
  }

  function stopFrame() {
    if (frame !== null) cancelAnimationFrame(frame);
    frame = null;
    previousFrameTime = null;
  }

  function tick(time: number) {
    frame = null;
    if (!duck || document.hidden || !isTrackingEnabled()) return;
    const dt =
      previousFrameTime === null ? 16 : time - previousFrameTime;
    previousFrameTime = time;
    const alpha = 1 - Math.exp(-dt / DAMPING_MS);
    currentYaw += (targetYaw - currentYaw) * alpha;
    currentPitch += (targetPitch - currentPitch) * alpha;
    writeSprites(currentYaw, currentPitch);

    if (
      Math.abs(targetYaw - currentYaw) > EPSILON ||
      Math.abs(targetPitch - currentPitch) > EPSILON
    ) {
      frame = requestAnimationFrame(tick);
    } else {
      currentYaw = targetYaw;
      currentPitch = targetPitch;
      writeSprites(currentYaw, currentPitch);
      previousFrameTime = null;
    }
  }

  function startFrame() {
    if (
      duck &&
      isTrackingEnabled() &&
      !document.hidden &&
      frame === null &&
      (Math.abs(targetYaw - currentYaw) > EPSILON ||
        Math.abs(targetPitch - currentPitch) > EPSILON)
    ) {
      previousFrameTime = null;
      frame = requestAnimationFrame(tick);
    }
  }

  function setTarget(yaw: number, pitch: number) {
    if (!duck || !isTrackingEnabled() || document.hidden) return;
    targetYaw = yaw;
    targetPitch = pitch;
    startFrame();
  }

  function measure() {
    if (!duck) return;
    duck.bounds = duck.element.getBoundingClientRect();
  }

  function onPointerMove(event: PointerEvent) {
    if (!duck || !isTrackingEnabled()) return;
    const { bounds } = duck;
    if (!bounds.width || !bounds.height) return;
    const dx = event.clientX - (bounds.left + bounds.width / 2);
    const dy = event.clientY - (bounds.top + bounds.height / 2);
    const yawRadius = Math.max(bounds.width * 1.2, 350);
    const pitchRadius = Math.max(bounds.height, 280);
    setTarget(
      clamp(dx / yawRadius, -1, 1) * 8,
      clamp(dy / pitchRadius, -1, 1) * 5,
    );
  }

  function onPointerOut(event: PointerEvent) {
    if (event.relatedTarget === null) setTarget(0, 0);
  }

  function onBlur() {
    setTarget(0, 0);
  }

  function onResize() {
    measure();
  }

  function resetToCenterImmediately() {
    stopFrame();
    targetYaw = currentYaw = 0;
    targetPitch = currentPitch = 0;
    writeSprites(0, 0);
  }

  function syncDuck() {
    const found = root.querySelector<HTMLElement>('#plec-duck-head');
    if (duck?.element === found) return;
    detachDuck();
    if (!found) return;

    const sprites = new Map<string, HTMLImageElement>();
    const opacity = new Map<string, number>();
    for (const image of found.querySelectorAll<HTMLImageElement>(
      'img[data-duck-yaw][data-duck-pitch]',
    )) {
      const yaw = Number(image.dataset.duckYaw);
      const pitch = Number(image.dataset.duckPitch);
      if (!Number.isFinite(yaw) || !Number.isFinite(pitch)) continue;
      const key = `${yaw}:${pitch}`;
      sprites.set(key, image);
      opacity.set(key, yaw === 0 && pitch === 0 ? 1 : 0);
    }

    const resizeObserver = new ResizeObserver(measure);
    duck = {
      element: found,
      sprites,
      opacity,
      bounds: found.getBoundingClientRect(),
      resizeObserver,
    };
    resizeObserver.observe(found);
    targetYaw = currentYaw = 0;
    targetPitch = currentPitch = 0;
    writeSprites(0, 0);
  }

  function detachDuck() {
    stopFrame();
    duck?.resizeObserver.disconnect();
    duck = null;
    targetYaw = currentYaw = 0;
    targetPitch = currentPitch = 0;
  }

  function onMotionPreferenceChange() {
    if (!isTrackingEnabled()) resetToCenterImmediately();
  }

  function onVisibilityChange() {
    if (document.hidden) stopFrame();
  }

  const observer = new MutationObserver(syncDuck);
  observer.observe(root, { childList: true, subtree: true });
  window.addEventListener('pointermove', onPointerMove, {
    passive: true,
  });
  window.addEventListener('pointerout', onPointerOut);
  window.addEventListener('blur', onBlur);
  window.addEventListener('resize', onResize, { passive: true });
  document.addEventListener('visibilitychange', onVisibilityChange);
  reducedMotion.addEventListener('change', onMotionPreferenceChange);
  coarsePointer.addEventListener('change', onMotionPreferenceChange);
  syncDuck();

  return () => {
    observer.disconnect();
    detachDuck();
    window.removeEventListener('pointermove', onPointerMove);
    window.removeEventListener('pointerout', onPointerOut);
    window.removeEventListener('blur', onBlur);
    window.removeEventListener('resize', onResize);
    document.removeEventListener(
      'visibilitychange',
      onVisibilityChange,
    );
    reducedMotion.removeEventListener(
      'change',
      onMotionPreferenceChange,
    );
    coarsePointer.removeEventListener(
      'change',
      onMotionPreferenceChange,
    );
  };
}
