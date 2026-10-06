import { useHostRef, useReaction, useRef } from '@plec/core';
import { SidebarHeader } from './sidebar-header';
import { SidebarNavigation } from './sidebar-navigation';

export function AppSidebar({
  collapsed,
  mobileOpen,
  onMobileToggle,
  onCloseMobile,
}: {
  collapsed: boolean;
  mobileOpen: boolean;
  onMobileToggle(): void;
  onCloseMobile(): void;
}) {
  const panelRef = useHostRef<HTMLElement>();
  const previousFocus = useRef<Element | null>(null);
  useReaction(() => {
    if (mobileOpen) {
      previousFocus.current = document.activeElement;
      panelRef.current?.focus();
    } else {
      // @ts-ignore: previousFocus.current might not be an HTMLElement
      previousFocus.current?.focus();
    }
  }, [mobileOpen]);

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
        ref={panelRef}
        className="fixed inset-y-2 left-2 z-20 flex w-72 -translate-x-[calc(100%+0.75rem)] flex-col overflow-hidden rounded-xl border border-sidebar-border bg-sidebar shadow-sm outline-none transition-[width,transform] duration-200 ease-in-out group-data-[mobile-open=true]:translate-x-0 md:w-64 md:translate-x-0 md:group-data-[collapsed=true]:w-14"
        tabIndex={-1}
      >
        <SidebarHeader />
        <SidebarNavigation onCloseMobile={onCloseMobile} />
      </aside>
    </div>
  );
}
