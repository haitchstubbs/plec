import {
  InfoCard,
  PageFrame,
  PageKicker,
} from '../components/page-primitives';
import { useLocation, useState } from '@plec/core';

export function HomePage() {
  const location = useLocation();
  const [count, setCount] = useState(0);
  return (
    <PageFrame>
      <PageKicker>Runtime control room</PageKicker>
      <h1 className="m-0 text-4xl font-bold tracking-tight sm:text-5xl">
        TSX enters as source. Plec owns the resulting DOM.
      </h1>
      <p className="m-0 max-w-2xl text-lg leading-8 text-muted-foreground">
        The home view is a compact operational snapshot of the
        experimental fullstack runtime.
      </p>
      <p id="ssr-request" className="m-0 text-sm text-muted-foreground">
        Requested {location.pathname}
        {location.search}
      </p>
      <button
        id="ssr-counter"
        type="button"
        onClick={() => setCount(count + 1)}
      >
        SSR counter: {count}
      </button>
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
    </PageFrame>
  );
}
