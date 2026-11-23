import { defineMessages } from '../i18n';

export const credentialRecoveryMessages = defineMessages(
  {
    "advancedDescription": "Your Advanced data credentials could not be saved. They stay in memory for up to 10 minutes so you can retry without signing in or entering the token again. Cancelling, starting a new login, resubmitting manual credentials or quitting clears them.",
    "advancedFileWarning": "This saves the Advanced data App Token as plain text in credentials.json in the app data folder. On Linux and macOS, the file uses permission 0600. Programs running as your user can read it. The app will reuse this file on restart unless your startup configuration selects another store.",
    "advancedDiscard": "Discard these login credentials",
    "advancedExpired": "These login credentials are no longer available. Please connect Advanced data again.",
    "title": "Choose how to save your login",
    "description": "Authorization succeeded, but the tokens could not be saved. They stay in memory for up to 10 minutes so you can retry without signing in again. Discarding this authorization, starting a new one or quitting clears them.",
    "systemOption": "Repair the system credential store (recommended)",
    "fileOption": "Save to a local JSON file",
    "repairLinux": "Open your system password or wallet manager. Create a default keyring or wallet if needed, and unlock it. On desktops such as Niri, make sure a Secret Service provider such as KWallet or GNOME Keyring is running. Then retry below.",
    "repairMac": "Open Keychain Access, unlock your login keychain and allow Zebra to save credentials. Then retry below.",
    "repairWindows": "Check that Windows Credential Manager is available and your account can use it. Then retry below.",
    "retrySystem": "Retry saving to the system store",
    "fileWarning": "This saves access and refresh tokens as plain text in credentials.json in the app data folder. On Linux and macOS, the file uses permission 0600. Programs running as your user can read it. The app will reuse this file on restart unless your startup configuration selects another store.",
    "consent": "I understand and agree to save the tokens in plain text on this device.",
    "saveFile": "Agree and save to JSON",
    "discard": "Discard this authorization",
    "expired": "This authorization is no longer available. Please sign in again.",
    "savedFile": "Login saved to local credentials.json.",
    "fileUnavailable": "File storage is unavailable on this platform or conflicts with your startup configuration. Repair the system credential store to continue."
  },
);
