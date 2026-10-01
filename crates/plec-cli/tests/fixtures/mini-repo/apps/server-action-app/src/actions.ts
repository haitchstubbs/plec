import { action } from '@plec/core';

export const echo = action(async (value: string) => {
  const privateMarker = 'PLEC_SERVER_ACTION_SECRET_7D3F';
  return { echoed: value, markerLength: privateMarker.length };
});
