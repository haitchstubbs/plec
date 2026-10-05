import { Workflow } from 'lucide';
import logoUrl from '../assets/plec-logo-transparent.png';

export function SidebarHeader() {
  return (
    <div>
      <div
        className="
          
          flex flex-nowrap items-center gap-3 p-2
          md:group-data-[collapsed=true]:justify-center
          md:group-data-[collapsed=true]:gap-0
          md:group-data-[collapsed=true]:p-2
        "
      >
        <div
          className="grid size-8 shrink-0 place-items-center rounded-lg bg-background text-sidebar-primary-foreground"
          aria-hidden="true"
        >
          <img src={logoUrl} alt="Plec Logo" width={24} height={24} />
        </div>

        <div className="w-max max-w-56 shrink-0 overflow-hidden md:group-data-[collapsed=true]:hidden">
          <div className="w-max">
            <strong className="block whitespace-nowrap text-xs">
              Plec Fullstack
            </strong>
            <span className="mt-0.5 block whitespace-nowrap text-xs text-muted-foreground">
              Experimental runtime
            </span>
          </div>
        </div>
      </div>
      <hr className="border-t border-sidebar-border" />
    </div>
  );
}
