export function PageFrame({ children }: { children?: unknown }) {
  return (
    <main className="min-h-[calc(100svh-4rem)] bg-background px-5 py-10 text-foreground sm:px-8">
      <section className="mx-auto grid max-w-4xl gap-6">
        {children}
      </section>
    </main>
  );
}

export function PageKicker({ children }: { children?: unknown }) {
  return (
    <p className="m-0 text-xs font-bold uppercase tracking-[.18em] text-primary">
      {children}
    </p>
  );
}

export function InfoCard({
  title,
  children,
}: {
  title: string;
  children?: unknown;
}) {
  return (
    <article className="rounded-xl border bg-card p-5 text-card-foreground shadow-sm [&_ol]:m-0 [&_ol]:grid [&_ol]:gap-2 [&_ol]:pl-5 [&_ol]:text-muted-foreground [&_p]:m-0 [&_p]:leading-relaxed [&_p]:text-muted-foreground">
      <h2 className="mb-2 text-base font-semibold">{title}</h2>
      <div>{children}</div>
    </article>
  );
}
