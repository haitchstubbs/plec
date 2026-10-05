import {
  InfoCard,
  PageFrame,
  PageKicker,
} from '../components/page-primitives';
import { useLocation, useMutation, useState } from '@plec/core';
import logoUrl from '../assets/plec-logo-transparent.png';
import { DuckHead } from './duck-head';
import { echoAction } from '../server-actions';

export function HomePage() {
  const location = useLocation();
  const [count, setCount] = useState(0);
  const [echoed, setEchoed] = useState('not called');
  const echo = useMutation(async (value: string) => {
    return await echoAction(value);
  });
  async function callEcho() {
    const result = await echo.run('browser-to-node');
    setEchoed(result.echoed);
  }
  return (
    <PageFrame>
      <div className="grid gap-6">
        <section className="relative overflow-hidden rounded-xl border bg-card p-5 text-card-foreground shadow-sm sm:p-5">
          <div className="absolute -right-10 -top-10 h-56 w-56 rounded-full bg-primary/5 blur-2xl" />
          <div className="relative flex flex-col gap-6 sm:flex-row sm:items-center sm:justify-between">
            <div className="max-w-2xl">
              <div className="mb-6 flex items-center gap-3">
                <img
                  id="compiled-asset-logo"
                  className="h-10 w-10 rounded-xl border bg-background p-1.5"
                  src={logoUrl}
                  alt="Plec mark"
                  width="40"
                  height="40"
                />
                <PageKicker>Runtime control room</PageKicker>
              </div>
              <h1 className="m-0 max-w-xl text-4xl font-bold leading-tight tracking-tight sm:text-5xl">
                TSX in. A living app out.
              </h1>
              <p className="mb-0 mt-4 max-w-xl text-base leading-7 text-muted-foreground sm:text-lg">
                Explore Plec’s experimental fullstack runtime, where
                compiled semantics drive precise updates to the DOM.
              </p>
            </div>
            <div className="flex justify-center sm:shrink-0">
              <DuckHead />
            </div>
          </div>
          <p
            id="ssr-request"
            className="relative mb-0 mt-8 border-t pt-4 text-xs text-muted-foreground"
          >
            Serving{' '}
            <span className="font-mono">
              {location.pathname}
              {location.search}
            </span>
          </p>
        </section>
        <section
          aria-label="Live demos"
          className="grid gap-4 md:grid-cols-2"
        >
          <InfoCard title="Live in the browser">
            <p>Click to update state without replacing the page.</p>
            <button
              id="ssr-counter"
              className="mt-4 rounded-lg bg-primary px-4 py-2 text-sm font-semibold text-primary-foreground transition-opacity hover:opacity-90"
              type="button"
              onClick={() => setCount(count + 1)}
            >
              SSR counter: {count}
            </button>
          </InfoCard>
          <InfoCard title="Connected to the server">
            <p>Send a request through the app’s server action.</p>
            <div className="mt-4 flex flex-wrap items-center gap-3">
              <button
                id="server-action-echo"
                className="rounded-lg border bg-background px-4 py-2 text-sm font-semibold transition-colors hover:bg-muted"
                type="button"
                onClick={callEcho}
                disabled={echo.pending}
              >
                {echo.pending
                  ? 'Calling server…'
                  : 'Call server action'}
              </button>
              <output
                id="server-action-result"
                className="text-sm text-muted-foreground"
              >
                {echoed}
              </output>
            </div>
          </InfoCard>
        </section>
        <div className="grid gap-4 md:grid-cols-2">
          <InfoCard title="Renderer">
            <p className="font-semibold !text-emerald-700 dark:!text-emerald-400">
              WASM client renderer ready
            </p>
          </InfoCard>
          <InfoCard title="Todo API">
            <p>
              GET /api/todos is available for the next data-graph
              experiment.
            </p>
          </InfoCard>
        </div>
        <InfoCard title="Current focus">
          <p>
            Keep the application graph small, inspectable, and able to
            update a precise DOM sink.
          </p>
        </InfoCard>
      </div>
    </PageFrame>
  );
}
