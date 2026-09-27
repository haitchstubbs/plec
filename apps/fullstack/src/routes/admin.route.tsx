import { createRoute, redirect } from '@plec/core';
import { Route as rootRoute } from './index';

type Session = { admin: boolean };

export const Route = createRoute<Session>({
  getParentRoute: () => rootRoute,
  path: 'admin',
  loader: async () => {
    const session = (await fetch('/api/session')) as Session;
    if (!session.admin) throw redirect('/notes');
    return session;
  },
  component: AdminPage,
  meta: {
    title: 'Admin',
    description: 'A loader redirect demonstration.',
  },
});

function AdminPage() {
  return (
    <section>
      <h1>Admin</h1>
      <p>
        Only admins see this page; the loader redirects everyone else.
      </p>
    </section>
  );
}
