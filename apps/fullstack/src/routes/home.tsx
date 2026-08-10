import { InfoCard, PageFrame, PageKicker } from "../components/page-primitives"

export function HomePage() {
  return (
    <PageFrame>
      <PageKicker>Runtime control room</PageKicker>
      <h1>TSX enters as source. O1 owns the resulting DOM.</h1>
      <p className="o1-page-lede">The home view is a compact operational snapshot of the experimental fullstack runtime.</p>
      <div className="o1-card-grid"><InfoCard title="Renderer"><p className="o1-status-good">WASM client renderer ready</p></InfoCard><InfoCard title="Todo API"><p>GET /api/todos is available for the next data-graph experiment.</p></InfoCard></div>
      <InfoCard title="Current focus"><p>Keep the application graph small, inspectable, and able to update a precise DOM sink.</p></InfoCard>
    </PageFrame>
  )
}
