import {
  InfoCard,
  PageFrame,
  PageKicker,
} from '../components/page-primitives';
export function AboutPage() {
  return (
    <PageFrame>
      <PageKicker>Architecture note</PageKicker>
      <h1 className="m-0 text-4xl font-bold tracking-tight sm:text-5xl">
        Plec is a renderer, not a React compatibility layer.
      </h1>
      <p className="m-0 max-w-2xl text-lg leading-8 text-muted-foreground">
        Constrained TSX becomes an inspectable application graph, then
        Rust/WASM creates and updates the browser DOM directly.
      </p>
      <InfoCard title="The path">
        <ol>
          <li>Source TSX becomes deterministic JSON IR.</li>
          <li>The persistent Plec layout exposes a route outlet.</li>
          <li>WASM replaces only the selected route graph.</li>
        </ol>
      </InfoCard>
      <InfoCard title="Why this matters">
        <p>
          Shared layout state and identity survive navigation without
          introducing a component renderer.
        </p>
      </InfoCard>
    </PageFrame>
  );
}
