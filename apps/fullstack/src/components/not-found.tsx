export function NotFoundPage() {
  return (
    <main className="grid min-h-screen place-items-center bg-background px-6 text-foreground">
      <section className="max-w-lg space-y-5 text-center">
        <p className="text-sm font-semibold uppercase tracking-[0.24em] text-rose-700">
          404
        </p>
        <h1 className="text-4xl font-bold tracking-tight">
          This route does not exist.
        </h1>
        <a
          className="inline-flex rounded-lg bg-sky-600 px-4 py-2 text-sm font-semibold text-white"
          href="/"
        >
          Return home
        </a>
      </section>
    </main>
  );
}
