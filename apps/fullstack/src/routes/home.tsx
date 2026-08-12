import {
  InfoCard,
  PageFrame,
  PageKicker,
} from '../components/page-primitives';

export function HomePage() {
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
