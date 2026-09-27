import { createRoute } from '@plec/core';
import { Route as rootRoute } from './index';

type Notes = { headline: string; detail: string };

export const Route = createRoute({
  getParentRoute: () => rootRoute,
  path: 'notes',
  loader: async () => {
    const response = await fetch('/api/notes');
    if (!response.ok) throw new Error('Could not load notes');
    return (await response.json()) as Notes;
  },
  errorComponent: NotesError,
  component: NotesPage,
  meta: {
    title: 'Server notes',
    description:
      'A loader route whose data the server transfers to the browser.',
  },
});

export function NotesError({ retry }: { retry?: () => void }) {
  return (
    <section>
      <p role="alert">Could not load notes.</p>
      <button type="button" onClick={retry}>
        Try again
      </button>
    </section>
  );
}

export function NotesPage() {
  const notes = Route.useLoaderData();
  return (
    <section>
      <h1>{notes.headline}</h1>
      <p>{notes.detail}</p>
    </section>
  );
}
