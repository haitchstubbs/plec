import { InfoCard, PageFrame, PageKicker } from "../components/page-primitives"

export function AboutPage() {
  return (
    <PageFrame tone="dark">
      <PageKicker>Architecture note</PageKicker>
      <h1>O1 is a renderer, not a React compatibility layer.</h1>
      <p className="o1-page-lede">Constrained TSX becomes an inspectable application graph, then Rust/WASM creates and updates the browser DOM directly.</p>
      <InfoCard title="The path"><ol><li>Source TSX becomes deterministic JSON IR.</li><li>The persistent O1 layout exposes a route outlet.</li><li>WASM replaces only the selected route graph.</li></ol></InfoCard>
      <InfoCard title="Why this matters"><p>Shared layout state and identity survive navigation without introducing a component renderer.</p></InfoCard>
    </PageFrame>
  )
}
