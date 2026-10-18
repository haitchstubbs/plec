import { action } from '@plec/core';
import { actionFailure, invalidate } from '@plec/core/server';
import { requestContext } from '@plec/core/server-context';
import { setProjectName } from './project-data';

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

export const renameProject = action(async (id: string, name: string) => {
  await new Promise((resolve) => setTimeout(resolve, 150));
  if (!setProjectName(id, name)) {
    actionFailure({ code: 'PROJECT_NOT_FOUND', message: 'Project not found.' });
  }
  invalidate(`project:${id}`);
  invalidate(`project:${id}`);
  return { id, name };
});
