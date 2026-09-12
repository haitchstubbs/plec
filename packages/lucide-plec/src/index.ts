export type { IconComponent, IconProps } from './create-icon.js';

import type { IconNode } from 'lucide';

export * from 'lucide';

type HostHandle = { element: SVGElement; attributes: Set<string> };
type IconDefinition = IconNode;

function setAttributes(
  element: Element,
  props: Record<string, unknown>,
) {
  for (const [key, value] of Object.entries(props)) {
    if (
      key === 'children' ||
      value == null ||
      typeof value === 'function'
    )
      continue;
    element.setAttribute(
      key === 'className' ? 'class' : key,
      String(value),
    );
  }
}

const defaultAttributes = {
  xmlns: 'http://www.w3.org/2000/svg',
  width: 24,
  height: 24,
  viewBox: '0 0 24 24',
  fill: 'none',
  stroke: 'currentColor',
  'stroke-width': 2,
  'stroke-linecap': 'round',
  'stroke-linejoin': 'round',
};

function attributeNames(props: Record<string, unknown>) {
  return new Set(
    Object.entries(props)
      .filter(
        ([key, value]) =>
          key !== 'children' &&
          value != null &&
          typeof value !== 'function',
      )
      .map(([key]) => (key === 'className' ? 'class' : key)),
  );
}

function escapeHtml(value: unknown): string {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function renderAttributes(props: Record<string, unknown>): string {
  return Object.entries(props)
    .filter(
      ([key, value]) =>
        key !== 'children' &&
        value != null &&
        typeof value !== 'function' &&
        /^[A-Za-z_:][A-Za-z0-9:._-]*$/.test(key) &&
        !key.toLowerCase().startsWith('on'),
    )
    .map(([key, value]) =>
      ` ${key === 'className' ? 'class' : key}="${escapeHtml(value)}"`,
    )
    .join('');
}

/** Node-safe SVG serialization used only by an SSR-opted-in provider. */
function renderIcon(
  definition: IconDefinition,
  props: Record<string, unknown>,
): string {
  const attributes = { ...defaultAttributes, ...props };
  const children = definition
    .map(([tag, childAttributes]) =>
      `<${tag}${renderAttributes(childAttributes)}></${tag}>`,
    )
    .join('');
  return `<svg${renderAttributes(attributes)}>${children}</svg>`;
}

function mountIcon(
  definition: IconDefinition,
  boundary: Element,
  props: Record<string, unknown>,
): HostHandle {
  const svg = document.createElementNS(
    'http://www.w3.org/2000/svg',
    'svg',
  );
  const attributes = { ...defaultAttributes, ...props };
  setAttributes(svg, attributes);
  for (const [tag, attributes] of definition) {
    const child = document.createElementNS(
      'http://www.w3.org/2000/svg',
      tag,
    );
    setAttributes(child, attributes);
    svg.append(child);
  }
  boundary.replaceChildren(svg);
  return { element: svg, attributes: attributeNames(attributes) };
}

export function createLucideHostProvider(
  definitions: Record<string, IconDefinition>,
) {
  const components: Record<
    string,
    {
      mount(
        boundary: Element,
        props: Record<string, unknown>,
      ): HostHandle;
      update(handle: HostHandle, props: Record<string, unknown>): void;
      dispose(handle: HostHandle): void;
      render(props: Record<string, unknown>): string;
    }
  > = {};
  for (const [name, definition] of Object.entries(definitions)) {
    components[name] = {
      mount: (boundary, props) =>
        mountIcon(definition, boundary, props),
      update: (handle, props) => {
        const attributes = { ...defaultAttributes, ...props };
        const nextNames = attributeNames(attributes);
        for (const name of handle.attributes) {
          if (!nextNames.has(name))
            handle.element.removeAttribute(name);
        }
        setAttributes(handle.element, attributes);
        handle.attributes = nextNames;
      },
      dispose: ({ element }) => element.remove(),
      render: (props) => renderIcon(definition, props),
    };
  }
  return components;
}

/** Build-generated provider entries pass only icons referenced by compiled IR. */
export function LucidePlecAdapter(
  definitions: Record<string, IconDefinition>,
) {
  return createLucideHostProvider(definitions);
}

export default LucidePlecAdapter;
