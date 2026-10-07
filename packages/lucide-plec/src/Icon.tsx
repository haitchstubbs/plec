import type { IconNode } from 'lucide';

export function Icon({
  icon,
  className,
}: {
  icon: IconNode;
  className?: string;
}) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="24"
      height="24"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width="2"
      stroke-linecap="round"
      stroke-linejoin="round"
      className={className}
      aria-hidden="true"
    >
      {icon.map(([tag, attributes]) => {
        switch (tag) {
          case 'path':
            return <path {...attributes} />;
          case 'circle':
            return <circle {...attributes} />;
          case 'line':
            return <line {...attributes} />;
          case 'polyline':
            return <polyline {...attributes} />;
          case 'polygon':
            return <polygon {...attributes} />;
          case 'rect':
            return <rect {...attributes} />;
          case 'ellipse':
            return <ellipse {...attributes} />;
          default:
            return null;
        }
      })}
    </svg>
  );
}