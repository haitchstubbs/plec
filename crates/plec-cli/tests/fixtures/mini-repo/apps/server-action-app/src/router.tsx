import { createRouter } from '@plec/core';
import { Route as rootRoute } from './routes/index';

export const router = createRouter({ routeTree: rootRoute });
