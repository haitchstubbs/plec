/** Declares an exported function for invocation from compiled Plec actions.
 * The compiler records only its opaque reference in browser artifacts; the
 * function itself remains ordinary JavaScript in the Node server bundle.
 */
export function action(implementation) {
    return implementation;
}
