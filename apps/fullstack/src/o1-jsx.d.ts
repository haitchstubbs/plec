/** TSX is compiler input for O1, not a call to a JavaScript JSX runtime. */
declare namespace JSX {
  interface IntrinsicElements {
    [elementName: string]: Record<string, unknown>;
  }
}
