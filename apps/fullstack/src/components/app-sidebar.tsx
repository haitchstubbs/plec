import { SidebarBrand, SidebarFrame, SidebarLink } from "../browser-primitives"

export function AppSidebar() {
  return <SidebarFrame>
    <SidebarBrand title="O1 Fullstack" version="Experimental runtime" />
    <nav className="o1-sidebar-nav" aria-label="Primary navigation">
      <SidebarLink href="/">Home</SidebarLink>
      <SidebarLink href="/about">About</SidebarLink>
    </nav>
  </SidebarFrame>
}
