import type { IconNode } from 'lucide';

type PlecNode = { type: string; props: Record<string, unknown> };
export type IconProps = Record<string, unknown>;
export type IconComponent = (props: IconProps) => PlecNode;

const node = (
  type: string,
  props: Record<string, unknown>,
): PlecNode => ({ type, props });

export function createIcon(definition: IconNode): IconComponent {
  return (props) =>
    node('svg', {
      viewBox: '0 0 24 24',
      fill: 'none',
      stroke: 'currentColor',
      strokeWidth: 2,
      strokeLinecap: 'round',
      strokeLinejoin: 'round',
      focusable: false,
      'aria-hidden': true,
      ...props,
      children: definition.map(([tag, attributes]) =>
        node(tag, attributes),
      ),
    });
}
