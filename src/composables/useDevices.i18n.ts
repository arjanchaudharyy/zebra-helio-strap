import { defineMessages } from '../i18n';

export const devicesMessages = defineMessages(
  {
    stateAccount: 'Known from account',
    stateUserAssigned: 'Model you picked',
    stateRecentData: 'Has recent data',
    stateCached: 'From cache',
    stateUnknown: 'Not identified',
    notFetchedYet: 'Not fetched yet',
    timeUnknown: 'Time unknown',
    unidentifiedDevice: 'Unidentified device',
    genericScale: 'Body composition scale',
    notProvided: 'Not provided',
    identifyUnavailable: 'Device identification unavailable right now',
    cacheUnavailable: 'Device cache unavailable right now',
    noLocalIdentifier: 'No local identifier on this device — the pick cannot be saved.',
    assignmentCleared: 'Pick withdrawn. Back to the automatic match.',
    assignmentSaved: 'Pick saved. Shown as "Model you picked" — never passed off as an automatic match.',
    assignmentFailed: 'Could not save the model pick',
    hideFailed: 'Could not hide or restore this device',
  },
);
