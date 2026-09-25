import { AppSidebar } from './app-sidebar';
import { ChevronRight, PanelLeft } from 'lucide';
import {
  useState,
  useListener,
  useLocation,
  Outlet,
  Link,
  cookie,
} from '@plec/core';

export function FullstackLayout() {
  const [collapsed, setCollapsed] = useState(
    cookie.getSync('sidebar_state') === 'false',
  );
  const [mobileOpen, setMobileOpen] = useState(false);
  const location = useLocation();
  const setSidebarCollapsed = (next: boolean) => {
    void cookie.set('sidebar_state', next ? 'false' : 'true', {
      path: '/',
      maxAge: 604800,
    });
    setCollapsed(next);
  };
  const closeMobile = () => setMobileOpen(false);

  useListener(window, 'keydown', (event) => {
    if ((event.metaKey || event.ctrlKey) && event.key === 'b') {
      event.preventDefault();
      setSidebarCollapsed(!collapsed);
    }
    if (event.key === 'Escape' && mobileOpen) {
      event.preventDefault();
      closeMobile();
    }
  });

  const page =
    location.pathname === '/'
      ? 'Home'
      : location.pathname === '/about'
        ? 'About'
        : location.pathname === '/todos'
          ? 'Todos'
          : location.pathname === '/notes'
            ? 'Notes'
            : location.pathname === '/stress'
              ? 'Runtime stress'
              : 'Not found';

  return (
    <div className="min-h-svh md:[&_.plec-sidebar-inset]:ml-[17rem] md:has-[[data-collapsed=true]]:[&_.plec-sidebar-inset]:ml-16">
      <AppSidebar
        collapsed={collapsed}
        mobileOpen={mobileOpen}
        onMobileToggle={() => {
          setMobileOpen(!mobileOpen);
        }}
        onCloseMobile={closeMobile}
      />
      <div className="plec-sidebar-inset min-h-svh bg-background transition-[margin] duration-200">
        <header className="flex h-16 items-center gap-2 border-b bg-background/80 px-4">
          <button
            type="button"
            onClick={() => setSidebarCollapsed(!collapsed)}
            className="hidden size-8 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground md:inline-grid"
            aria-label="Toggle sidebar"
            aria-expanded={!collapsed}
          >
            <PanelLeft className="size-4" />
            <span className="sr-only">Toggle sidebar</span>
          </button>
          <button
            type="button"
            onClick={() => setMobileOpen(!mobileOpen)}
            className="inline-grid size-8 place-items-center rounded-md text-muted-foreground hover:bg-muted hover:text-foreground md:hidden"
            aria-label="Toggle navigation"
            aria-expanded={mobileOpen}
          >
            <PanelLeft className="size-4" />
            <span className="sr-only">Toggle navigation</span>
          </button>
          <div className="mr-2 h-4 w-px bg-border" aria-hidden="true" />
          <nav aria-label="Breadcrumb">
            <ol className="flex items-center gap-2 text-sm text-muted-foreground">
              <li className="hidden md:block">
                <Link
                  className="no-underline hover:text-foreground"
                  to="/"
                >
                  Plec Fullstack
                </Link>
              </li>
              <li className="hidden md:block" aria-hidden="true">
                <ChevronRight className="block size-3.5" />
              </li>
              <li
                aria-current="page"
                className="font-medium text-foreground"
              >
                {page}
              </li>
            </ol>
          </nav>
        </header>
        <Outlet
          id="main"
          className="min-h-[calc(100svh-4rem)]"
          aria-live="polite"
        />
      </div>
    </div>
  );
}
