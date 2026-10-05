import type { IconNode } from 'lucide';

export function Icon({icon, className}: {icon: IconNode, className?: string}) {
    const props = icon[1]; 
    return <svg {...icon} className={className} />;
}
