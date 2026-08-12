import { CircleHelp } from '@wasm-runtime/lucide-plec/icons/circle-help';
import { House } from '@wasm-runtime/lucide-plec/icons/house';
import { ListChecks } from '@wasm-runtime/lucide-plec/icons/list-checks';
import { Workflow } from '@wasm-runtime/lucide-plec/icons/workflow';
import { Link as PlecLink } from 'plec';

export function AppSidebar({
  collapsed,
  mobileOpen,
  onDesktopToggle,
  onMobileToggle,
  onCloseMobile,
  pathname,
}: {
  collapsed: boolean;
  mobileOpen: boolean;
  onDesktopToggle(): void;
  onMobileToggle(): void;
  onCloseMobile(): void;
  pathname: string;
}) {
  const NavLink = ({
    href,
    Icon,
    children,
  }: {
    href: string;
    Icon: typeof House;
    children?: unknown;
  }) => (
    <PlecLink
      to={href}
      onClick={onCloseMobile}
      aria-current={pathname === href ? 'page' : undefined}
      className="flex items-center gap-3 rounded-lg px-3 py-2 text-sm font-medium no-underline transition-colors hover:bg-sidebar-accent hover:text-sidebar-accent-foreground aria-[current=page]:bg-sidebar-accent aria-[current=page]:text-sidebar-accent-foreground md:data-[collapsed=true]:justify-center md:data-[collapsed=true]:px-2"
    >
      <Icon className="size-4 shrink-0" />
      <span className="md:data-[collapsed=true]:hidden">
        {children}
      </span>
    </PlecLink>
  );
  return (
    <div
      className="group text-sidebar-foreground"
      data-collapsed={collapsed}
      data-mobile-open={mobileOpen}
    >
      <button
        type="button"
        onClick={onMobileToggle}
        className="hidden"
        aria-label="Open navigation"
        aria-expanded={mobileOpen}
      >
        ☰
      </button>
      <button
        type="button"
        onClick={onCloseMobile}
        className="fixed inset-0 z-[18] hidden border-0 bg-black/40 group-data-[mobile-open=true]:block md:hidden"
        aria-label="Close navigation"
        aria-hidden={!mobileOpen}
      />
      <aside
        className="fixed inset-y-2 left-2 z-20 flex w-72 -translate-x-[calc(100%+0.75rem)] flex-col gap-4 overflow-hidden rounded-xl border border-sidebar-border bg-sidebar shadow-sm outline-none transition-[width,transform] duration-200 group-data-[mobile-open=true]:translate-x-0 md:w-64 md:translate-x-0 md:group-data-[collapsed=true]:w-12"
        tabIndex={-1}
      >
        <button
          type="button"
          onClick={onDesktopToggle}
          className="absolute right-0 top-1/2 hidden h-16 w-2 -translate-y-1/2 cursor-ew-resize rounded-l bg-transparent hover:bg-sidebar-border md:block"
          aria-label="Toggle sidebar"
          aria-expanded={!collapsed}
        />
        <div className="flex items-center gap-3 p-3 md:group-data-[collapsed=true]:justify-center md:group-data-[collapsed=true]:p-2">
          <div
            className="grid size-8 shrink-0 place-items-center rounded-lg bg-sidebar-primary text-sidebar-primary-foreground"
            aria-hidden="true"
          >
            <Workflow className="size-4" />
          </div>
          <div className="md:group-data-[collapsed=true]:hidden">
            <strong className="block text-sm">Plec Fullstack</strong>
            <span className="mt-0.5 block text-xs text-muted-foreground">
              Experimental runtime
            </span>
          </div>
        </div>
        <nav
          className="flex flex-col gap-1 p-2"
          aria-label="Primary navigation"
        >
          <NavLink href="/" Icon={House}>
            Home
          </NavLink>
          <NavLink href="/about" Icon={CircleHelp}>
            About
          </NavLink>
          <NavLink href="/todos" Icon={ListChecks}>
            Todos
          </NavLink>
        </nav>
      </aside>
    </div>
  );
}
