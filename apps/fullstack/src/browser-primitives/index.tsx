/**
 * These are compiler input components, not React components. Their markers are
 * recorded in O1 layout metadata and the browser capability operates only on
 * the WASM-created elements identified by that metadata.
 */
export function RouteOutlet({ id = "main" }: { id?: string }) {
  return <main data-o1-route-outlet={id} className="o1-route-outlet" aria-live="polite" />
}

export function SidebarFrame({ children }: { children?: unknown }) {
  return <div data-o1-sidebar="root" data-o1-sidebar-persistence="sidebar_state" className="o1-sidebar-frame" data-state="expanded" data-mobile-open="false">
    <button type="button" data-o1-sidebar="mobile-toggle" className="o1-sidebar-mobile-toggle" aria-label="Open navigation" aria-expanded="false">☰</button>
    <button type="button" data-o1-sidebar="backdrop" className="o1-sidebar-backdrop" aria-label="Close navigation" aria-hidden="true" />
    <aside data-o1-sidebar="panel" className="o1-sidebar-panel" tabIndex="-1">
      <button type="button" data-o1-sidebar="desktop-toggle" className="o1-sidebar-rail" aria-label="Toggle sidebar" aria-expanded="true" />
      {children}
    </aside>
  </div>
}

export function SidebarBrand({ title, version }: { title: string; version: string }) {
  return <div className="o1-sidebar-brand">
    <div className="o1-sidebar-mark" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"><path d="M4 5h16v14H4z" /><path d="M8 9h8M8 13h5" /></svg></div>
    <div><strong>{title}</strong><span>{version}</span></div>
  </div>
}

export function SidebarLink({ href, children }: { href: string; children?: unknown }) {
  return <a href={href} data-o1-sidebar-link="true" className="o1-sidebar-link">{children}</a>
}
