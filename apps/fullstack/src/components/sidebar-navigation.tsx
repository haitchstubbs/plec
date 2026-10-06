import {
  Beaker,
  CircleHelp,
  FolderKanban,
  House,
  ListChecks,
  NotebookText,
  Shield,
} from 'lucide';
import { SidebarNavLink } from './sidebar-nav-link';

export function SidebarNavigation({
  onCloseMobile,
}: {
  onCloseMobile: () => void;
}) {
  return (
    <nav
      className="flex w-full flex-col gap-1 p-2"
      aria-label="Primary navigation"
    >
      <SidebarNavLink
        href="/"
        Icon={House}
        onCloseMobile={onCloseMobile}
      >
        Home
      </SidebarNavLink>
      <SidebarNavLink
        href="/about"
        Icon={CircleHelp}
        onCloseMobile={onCloseMobile}
      >
        About
      </SidebarNavLink>
      <SidebarNavLink
        href="/todos"
        Icon={ListChecks}
        onCloseMobile={onCloseMobile}
      >
        Todos
      </SidebarNavLink>
      <SidebarNavLink
        href="/notes"
        Icon={NotebookText}
        onCloseMobile={onCloseMobile}
      >
        Notes
      </SidebarNavLink>
      <SidebarNavLink
        href="/admin"
        Icon={Shield}
        onCloseMobile={onCloseMobile}
      >
        Admin
      </SidebarNavLink>
      <SidebarNavLink
        href="/projects/plec"
        Icon={FolderKanban}
        onCloseMobile={onCloseMobile}
      >
        Projects
      </SidebarNavLink>
      <SidebarNavLink
        href="/projects/ghost"
        Icon={FolderKanban}
        onCloseMobile={onCloseMobile}
      >
        Missing project
      </SidebarNavLink>
      <SidebarNavLink
        href="/stress"
        Icon={Beaker}
        onCloseMobile={onCloseMobile}
      >
        Runtime stress
      </SidebarNavLink>
    </nav>
  );
}
