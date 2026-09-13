export function Badge({
  title,
  children,
}: {
  title: string;
  children?: unknown;
}) {
  return <span data-badge={title}>{children}</span>;
}
