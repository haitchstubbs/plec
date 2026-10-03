import { action } from '@plec/core';
import { requestContext } from '@plec/core/server-context';

export const echoAction = action(async (value: string) => {
  const hasExpectedSession =
    requestContext().cookies.session === 'action-session';
  const privateMarker = 'PLEC_SERVER_ACTION_SECRET_7D3F';
  return {
    echoed: value,
    markerLength: privateMarker.length,
    hasExpectedSession,
  };
});
