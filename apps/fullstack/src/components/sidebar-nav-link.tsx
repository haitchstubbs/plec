import { Link, useLocation } from '@plec/core';
import { House } from 'lucide';

export function SidebarNavLink({
  href,
  Icon,
  children,
  onCloseMobile,
}: {
  href: string;
  Icon: typeof House;
  onCloseMobile: () => void;
  children: string;
}) {
  const location = useLocation();
  return (
    <Link
      to={href}
      onClick={onCloseMobile}
      aria-current={location.pathname === href ? 'page' : undefined}
      className="
        flex w-full items-center gap-3 rounded-lg px-2 py-2 text-sm font-medium no-underline
        transition-[width] duration-200 ease-in-out hover:bg-sidebar-accent hover:text-sidebar-accent-foreground
        aria-[current=page]:bg-sidebar-accent aria-[current=page]:text-sidebar-accent-foreground
        md:group-data-[collapsed=true]:size-9 md:group-data-[collapsed=true]:shrink-0 md:group-data-[collapsed=true]:p-0 md:group-data-[collapsed=true]:justify-center
        md:group-data-[collapsed=true]:gap-0
      "
    >
      <Icon className="size-4 shrink-0" />
      <span className="block w-max max-w-40 shrink-0 overflow-hidden whitespace-nowrap md:group-data-[collapsed=true]:hidden">
        {children}
      </span>
    </Link>
  );
}
