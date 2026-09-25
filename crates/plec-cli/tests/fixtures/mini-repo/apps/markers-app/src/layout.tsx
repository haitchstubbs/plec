import { Outlet, useLocation } from '@plec/core';
import { Badge } from './badge';

export function Layout() {
  const location = useLocation();
  const isList = location.pathname === '/list';
  return (
    <div>
      <Badge title={isList ? 'list' : 'page'}>shell</Badge>
      {isList ? <p>listing</p> : <p>browsing</p>}
      <Outlet id="main" />
    </div>
  );
}
