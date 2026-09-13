type Row = { id: string; label: string };

export function ListPage() {
  const rows: Row[] = [
    { id: 'one', label: 'One' },
    { id: 'two', label: 'Two' },
  ];
  return (
    <ul>
      {rows.map((row) => (
        <li key={row.id}>
          {row.label}
        </li>
      ))}
    </ul>
  );
}
