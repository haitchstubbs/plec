import { action } from '@plec/core';
import { actionFailure } from '@plec/core/server';
import { requestContext } from '@plec/core/server-context';

export const echoAction = action(async (value: string) => {
  const hasExpectedSession =
    requestContext().cookies.session === 'action-session';
  if (!hasExpectedSession) {
    actionFailure({
      code: 'SESSION_REQUIRED',
      message: 'Action was rejected.',
    });
  }
  const privateMarker = 'PLEC_SERVER_ACTION_SECRET_7D3F';
  return {
    echoed: value,
    markerLength: privateMarker.length,
    hasExpectedSession,
  };
});
