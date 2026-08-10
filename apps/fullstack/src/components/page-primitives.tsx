export function PageFrame({ children, tone = "light" }: { children?: unknown; tone?: string }) {
  return <main className={`o1-page o1-page-${tone}`}><section className="o1-page-content">{children}</section></main>
}

export function PageKicker({ children }: { children?: unknown }) { return <p className="o1-page-kicker">{children}</p> }

export function InfoCard({ title, children }: { title: string; children?: unknown }) {
  return <article className="o1-info-card"><h2>{title}</h2><div>{children}</div></article>
}
