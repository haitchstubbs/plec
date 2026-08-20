import type { PlecRouteManifest } from './index';
import { PlecRouteManifestSchema } from './index';

export function validatePlecRouteManifest(
  value: unknown,
): PlecRouteManifest {
  return PlecRouteManifestSchema.parse(value);
}

export type { PlecRouteManifest } from './index';
