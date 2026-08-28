import { createRootRoute } from 'plec';
import { FullstackLayout } from '../components/fullstack-layout';

export const Route = createRootRoute({
  component: FullstackLayout,
  meta: {
    title: 'Plec fullstack playground',
    description: 'Plec fullstack runtime experiment.',
  },
});
