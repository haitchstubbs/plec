import { currentRendering } from '../client/root/render-context';

export interface PlecLocation {
  pathname: string;
  search: string;
  hash: string;
}

export function useLocation(): PlecLocation {
  if (!currentRendering())
    throw new Error(
      'Plec.useLocation can only run while rendering a Plec component.',
    );
  return {
    pathname: window.location.pathname,
    search: window.location.search,
    hash: window.location.hash,
  };
}
