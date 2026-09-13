#!/usr/bin/env node
/** Builds the package/runtime surface used by monorepo applications and E2E. */
import { buildWorkspaceSurface } from './build-package.mjs';

await buildWorkspaceSurface();
