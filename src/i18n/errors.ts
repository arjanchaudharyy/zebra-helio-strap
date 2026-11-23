import { defineMessages, messagesOf } from './index';

const messages = defineMessages(
  {
    /* —— core —— */
    'err.core.network': "Could not reach the Zepp region — check your network and retry",
    'err.core.needs_reauth': 'Sign-in expired — reconnect to Zepp',
    'err.core.unavailable': "This account or region does not provide that data",
    'err.core.retry_exhausted': 'Zepp temporarily unavailable — retry shortly',
    'err.core.http_status': 'Zepp returned an error — retry shortly',
    'err.core.cloud_rejected':
      'Zepp received the request but refused it. If it keeps happening, reconnect the Zepp account in Settings',
    'err.core.cancelled': 'Cancelled',
    'err.core.auth': 'Authentication error',
    'err.headless.no_credential_store':
      'No system credential store on this machine (GNOME Keyring / KWallet) — headless servers and containers usually have none. '
      + 'Set ZEPPBRIDGE_CREDENTIAL_STORE=file to write the token 0600 into the data directory, '
      + 'or ZEPPBRIDGE_CREDENTIAL_STORE=env with ZEPPBRIDGE_APP_TOKEN.',
    'err.headless.schema_upgrade':
      'The local database is older than this build, and a read-only connection cannot upgrade it. '
      + 'Launch the desktop app once, or run zeppbridge-cli reprocess on the headless machine — '
      + 'both back up before upgrading.',
    'err.headless.program_outdated':
      'The CLI / MCP reading the database is older than the desktop app and cannot read this database version. '
      + 'Usually an AI tool still points at an old copy: reconnect that tool in Settings '
      + '(for Claude Desktop, generate and install ZeppBridge.mcpb again), remove the old extension '
      + 'or any entry pointing at an old install folder, then restart the AI tool.',
    'err.push.config_unreadable':
      'The push target configuration (push/targets.json in the data folder) could not be read. Check its file permissions and try again.',
    'err.push.config_invalid':
      'The push target configuration is malformed: target ids use lowercase letters, digits and hyphens and must be unique, and retention days must be 1–365.',
    'err.push.config_has_secret':
      'The push target configuration contains a secret field. Secrets belong in the credential store, not in the file; remove that field and enter the secret again.',
    'err.push.credential_store_unnamed':
      'The current credential store (environment-variable mode) holds a single token and cannot store push target secrets. Use the system credential store or ZEPPBRIDGE_CREDENTIAL_STORE=file.',
    'err.push.credential_store_failed':
      'The push target secret could not be read from or written to the credential store. Check that it is unlocked and allowed by system policy.',
    'err.push.secrets_missing':
      'The credential store has no complete secret for this push target. Enter the secret again.',
    'err.push.pa_write_key_malformed':
      'The write key is malformed. Copy the full write key from your site again.',
    'err.push.pa_tenant_mismatch':
      'The write key does not match the site id: it belongs to another site.',
    'err.push.pa_data_key_malformed':
      'The data key is malformed (it should be 32 bytes). Copy the full data key from your site again.',
    'err.push.site_invalid':
      'The push address cannot be used: it must be an https address with a domain name, not an IP address, and must not contain a user name, password, ? or #.',
    'err.push.connection_code_invalid':
      'That connection code doesn\'t look right. Paste the whole code; it should contain both dk and writeKey.',
    'err.push.not_configured':
      'No push destination has been set up yet.',
    'err.push.companion_not_found':
      'No Zebra Cloud setup to take over was found on this computer (it needs a site address and its upload key in Credential Manager).',
    'err.push.state_unwritable':
      'Push records could not be written to the data folder (push/). Check that the data folder is writable.',
    'err.push.no_local_data':
      'There is no local database yet, so there is nothing to push. Sync first.',
    'err.push.local_data_unavailable':
      'The local database could not be read right now. Nothing was pushed this round; it will try again after the next sync.',
    'err.push.coordinates_found':
      'Coordinates appeared in the content to be pushed, so this round was stopped. Routes are never pushed by default; please report this to the author.',
    'err.push.upload_failed':
      'Pushing to your site did not complete. What did not go through will be retried after the next sync.',
    'err.headless.token_not_in_store':
      "Account details are here, but the credential store has no token for them. "
      + "A database copies between machines; a token does not — it lives in the original "
      + "machine's credential store. Sign in again.",
    'err.core.credential_store':
      'Could not access the credential store. Check it is unlocked, allowed by system policy, '
      + 'correctly configured and has the right file permissions. Web sign-in and manual entry '
      + 'use the same store — switching methods cannot bypass a storage failure. '
      + 'If macOS Keychain or the Linux keyring is unavailable, follow the credential storage guide '
      + 'in the README: launch with ZEPPBRIDGE_CREDENTIAL_STORE=file, then sign in again. '
      + 'This saves the token as plaintext readable only by your user.',
    'err.core.invalid_host': 'Unsafe Zepp region address',
    'err.core.config': 'Configuration needs fixing first',
    'err.core.timed_out': 'This sync ran out of time; the rest will continue next time',
    'err.core.busy': 'Another write is in progress — wait for it to finish',
    'err.core.account_mismatch':
      'This database already holds another Zepp account\'s data, so nothing was written for this account. To switch accounts, quit Zebra, move or rename the data folder, then connect again',
    'err.core.parse': "Could not parse Zepp's response",
    'err.core.database': 'Local database temporarily unavailable',
    'err.core.io': 'Local file read/write failed',
    'err.core.unknown': 'Something went wrong',
    'err.app.starting': 'Local data is still being prepared — this will refresh on its own shortly',

    /* —— connect & auth —— */
    'err.auth.sync_init_failed': "Could not init sync — check the account region and retry",
    'err.auth.fresh_start_failed': 'Could not prepare a new data folder; your current data is unchanged',
    'err.auth.verify_network':
      "Verification failed: could not reach Zepp — check your network and retry",
    'err.auth.verify_needs_reauth':
      'Verification failed: credentials expired — save them again',
    'err.auth.verify_failed': 'Verification failed',

    /* —— web login —— */
    'err.login.waiting': 'Finish signing in to Zepp in the pop-up window',
    'err.login.fallback_page': 'Opening the alternate sign-in page',
    'err.login.extracting': 'Credentials read. Confirming your region',
    'err.login.verifying': 'Verifying the account',
    'err.login.connected': 'Connected to your Zepp account',
    'err.official.browser': 'Couldn’t open your browser. Choose “Copy authorization link”, paste it into your browser’s address bar and approve there — the app connects on its own.',
    'err.open_url.failed': "Could not open the system browser — check your default browser and retry.",
    'err.open_url.not_allowed': 'Only https links can be opened.',
    'err.letters.client_init_failed': 'Could not set up the connection for sending letters. Restart the app and try again.',
    'err.letters.save_failed': 'Your reply could not be saved on this computer. Check free disk space and retry.',
    'err.letters.attachment_invalid': 'One of the images could not be read or is too large. Try another one.',
    'err.letters.invalid': 'This reply points at the wrong version and was not sent.',
    'err.letters.empty': 'Your reply is empty — write a few words first.',
    'err.letters.capsule_date': 'That opening date isn’t valid. Pick it again.',
    'err.feedback.toast_failed': 'The system notification could not be shown.',
    'err.export.my_records': 'Couldn’t write that file. Try another location.',
    'err.export.unstrap': 'Couldn’t write the file for unstrap. Check that the data folder can be written to.',
    'err.import.source': 'Unknown import source.',
    'err.import.too_large': 'That file is too large (over 64 MB). For Strava and Apple Health, pick the whole export archive.',
    'err.import.not_recognized': 'That file isn’t an export from the source you chose. Try another source.',
    'err.import.read': 'Couldn’t read that file. Make sure no other program has it open.',
    'err.ai_task.bad_prompt_file': 'That isn’t a usable .zbprompt file (wrong format, from a newer version, or no prompt).',
    'err.ai_task.prompt_export': 'Couldn’t export the .zbprompt. Check that the desktop is writable.',
    'err.ai_task.prompt_import': 'Couldn’t read that file (too large or can’t be opened).',
    'err.letters.rejected': 'The server did not accept this reply. You may need to update the app.',
    'err.mcp_writes.stale': 'This record was changed again afterwards, so it can’t be rolled back to how it was before the AI change. Edit it on its own page.',
    'err.mcp_writes.missing': 'This change was already undone, or it can’t be found.',
    'err.mcp.cli_missing': 'zeppbridge-cli is not next to the MCP program, so the AI can’t sync for you. Sync from Zebra instead.',
    'err.official.denied': 'You did not grant access — Zebra received nothing.',
    'err.official.rejected': 'Zepp did not accept this authorization — retry.',
    'err.official.expired': 'This authorization expired — start it again.',
    'err.official.timeout': 'Authorization timed out — start it again.',
    'err.official.not_enabled': 'Zepp authorization service not enabled yet — try again later.',
    'err.official.store': 'Authorized, but the token could not be saved to the system credential store.',
    'err.official.failed': 'Zepp authorization did not complete — try again later.',
    'err.login.timeout': 'Sign-in timed out. Try again',
    'err.login.credentials_unreadable':
      "Signed in, but credentials could not be read from the sign-in window. Enter an App Token manually instead.",
    'err.login.region_probe_failed':
      "Credentials read, but the account region could not be confirmed. Sign in again, or enter an App Token manually.",
    'err.login.credentials_rejected':
      'Zepp rejected these credentials. Sign out in the login window, then sign in again',
    'err.login.region_unreachable':
      "Could not reach the Zepp region service — check your network and retry",
    'err.login.region_retrying':
      "Cannot reach the Zepp region service — retrying. The sign-in window stays open; no need to sign in again",
    'err.login.third_party_blocked':
      'Advanced data only supports signing in with a Zepp email or phone number and password — Google, Apple, WeChat and Xiaomi sign-in don’t work here. If you sign in to Zepp with one of those, use official authorization instead.',
    'err.login.other_account':
      'This is a different Zepp account: the account you just signed in with isn’t the one you authorized officially, so nothing was saved. If you created your Zepp account with Google, Apple or another third party, signing in with the same email and a password counts as a new Zepp account.',
    'err.official.other_account':
      'This is a different Zepp account: the account you authorized isn’t the one signed in for advanced data, so this authorization wasn’t saved. If you created your Zepp account with Google, Apple or another third party, signing in with the same email and a password counts as a new Zepp account.',
    'err.official.relay_unreachable':
      'Your browser may already show the authorization as done, but Zebra can’t reach zeppbridge.com to collect it. If your browser uses a proxy and the app doesn’t, turn on the system proxy (or let your proxy tool cover all apps), then choose “Check now”.',
    'err.official.still_pending':
      'Just checked: the relay hasn’t received this authorization yet. Make sure you approved it in the browser and saw the success page. If that page came from an earlier attempt, start the authorization again.',
    'err.official.browser_not_opened':
      'The authorization page doesn’t seem to have opened. If no browser window appeared, choose “Copy authorization link” and paste it into your browser’s address bar — the app connects on its own.',
    'err.backfill.start_too_early':
      'Backfill start is too early — at most the last 3650 days can be backfilled',
    'err.login.bad_url': 'Invalid sign-in address',
    'err.login.window_failed': "Could not open sign-in window",
    'err.login.window_busy':
      'The previous sign-in window is still closing — wait a moment and retry',
    'err.login.state_unavailable': 'App state unavailable',
    'err.login.cancelled': 'Sign-in cancelled',
    'err.login.sync_init_failed': "Signed in, but sync could not be initialized",

    /* —— sync & backfill —— */
    'err.sync.not_connected': 'Not connected to Zepp yet — connect first',
    'err.sync.not_verified': 'Verify the connection before syncing recent data',
    'err.sync.not_verified_probe': 'Verify the connection before probing capabilities',
    'err.sync.not_verified_backfill': 'Verify the connection before backfilling history',
    'err.sync.history_days_out_of_range': 'Sync days outside the allowed range',
    'err.sync.deferred_compaction':
      'Compacting stored payloads to save disk space — this sync retries automatically',
    'err.sync.deferred_replay':
      'Rebuilding derived data from local payloads — this sync retries automatically',
    'err.sync.deferred_busy':
      'Another write is in progress — this sync retries automatically',
    'err.local.maintenance_compaction': 'Compacting stored payloads — retries automatically when done',
    'err.local.maintenance_replay': 'Rebuilding data from local payloads — retries automatically when done',
    'err.backfill.bad_start_date': 'Invalid backfill start date — needs YYYY-MM-DD',
    'err.backfill.no_canonical_records':
      'The cloud returned a payload, but no usable records were parsed',
    'err.backfill.partial_window':
      'Only part of this window was written — a retry is still needed',
    'err.backfill.start_in_future': 'Backfill start cannot be later than today',
    'err.backup.restore_busy':
      'Restore did not run — another write is in progress. Current library unchanged; retried at next launch',
    'err.backup.restore_failed':
      'Restore did not finish. Current library unchanged; retried at next launch',
    'err.backup.not_found': 'Backup record does not exist or was deleted',

    /* —— capabilities —— */
    'err.capability.not_synced': 'Not synced yet',
    'err.capability.needs_reauth': 'Needs re-authentication',
    'err.capability.unverified': 'Not verified yet',
    'err.capability.unavailable': 'Unavailable',
    'err.capability.unknown': 'Status unknown',
    'err.capability.other': 'Status unknown',

    /* —— export —— */
    'err.export.empty_range': 'No records in this range to export',
    'err.export.convert_failed': "Could not convert to the requested format",
    'err.export.write_failed': "Could not write the export file",
    'err.mcp.sidecar_missing': 'This build does not include the MCP program; download zeppbridge-tools from GitHub',
    'err.mcp.bundle_failed': 'Could not create the Claude Desktop extension',
    'err.export.path_required': 'Pick a save location first',
    'err.export.path_not_absolute': 'Save location must be an absolute path',
    'err.export.not_a_directory':
      'FIT export needs a folder — the selected path is a file',

    /* —— hand to AI —— */
    'err.handoff.prompt_required': 'Write a prompt first',
    'err.handoff.empty_range': 'No records in this range to hand off',
    'err.handoff.mkdir_failed': "Could not create the export folder",
    'err.handoff.write_failed': "Could not write the de-identified AI data",
    'err.handoff.encode_failed': "Could not encode the de-identified AI export",

    /* —— feedback —— */
    'err.diagnostic.empty_report':
      "Pick a problem type or write a note first — otherwise the report has nothing actionable",
    'err.diagnostic.client_init_failed': "Could not initialize the report connection",
    'err.diagnostic.send_failed': "Could not send the report — check your network and retry",
    'err.diagnostic.http_error': 'The report service returned an error',
    'err.diagnostic.rate_limited':
      'Too many reports in a short time — retry later. Ones already sent are kept; no need to resend.',
    'err.diagnostic.bad_response': 'The report service returned an unreadable response',

    /* —— misc —— */
    'err.workout.not_found': 'Workout record does not exist',
    'err.workout_fit.not_fit': 'This is not a .fit file Zebra can read',
    'err.workout_fit.too_large': 'This file is too large to be a single workout .fit',
    'err.workout_fit.read_failed': 'Could not read this file',
    'err.workout_fit.no_activity': 'This .fit has no activity in it (no records and no laps)',
    'err.workout_fit.start_mismatch': 'This .fit starts more than 5 minutes away from this workout — it does not look like the same session',
    'err.workout_fit.original_missing': 'The original workout details are missing. An earlier backup or the original activity file may contain them.',
    'err.workout_fit.multiple_activities': 'Use a FIT file with one session to correct this workout',
    'err.workout.gpx_clipboard': 'Could not copy GPX to the clipboard',
    'err.workout.gpx_no_route': 'This workout has no GPS track points to export',
    'err.prefs.retention_out_of_range': 'Retention must be between 1 and 365 days',
    'err.storage.write_busy': 'Another Zebra write is in progress — wait for it to finish',
    'err.storage.write_lock_unavailable':
      "Could not create the write lock — check data folder permissions",
    'err.storage.worker_failed': 'Background database task was interrupted',
    'err.local_api.token_unavailable': "Could not read the local API credential",
    'err.local_api.token_rotate_failed': "Could not regenerate the local API credential",
    'err.local_api.port_in_use': 'Local API port already in use by another program',
    'err.local_api.bind_failed': "Could not start the local API",
    'err.local_api.thread_failed': "Could not start the local API thread",
    'err.local_api.state_write_failed': "Could not save the local API on/off state",
    'err.data_folder.open_failed': "Could not open the data folder",
    'err.data_folder.unsupported_os': 'Opening the data folder supports Windows/macOS only',
    'err.cloud.unsupported': 'The cloud component only works on Windows',
    'err.cloud.not_configured': 'The cloud component has no settings yet. Enter a site and save first.',
    'err.cloud.site_invalid': 'That site isn’t right: enter a bare domain (like example.com) or a full https:// address',
    'err.cloud.days_invalid': 'Days kept and summary days must be between 1 and 365',
    'err.cloud.bin_dir_invalid': 'That folder doesn’t contain zeppbridge-cli',
    'err.cloud.data_dir_invalid': 'The data folder must be the full path of a folder that exists',
    'err.cloud.uploader_not_found': 'Couldn’t find the cloud component’s upload scripts (run.ps1, setup.ps1, upload.mjs)',
    'err.cloud.config_unreadable': 'The cloud component’s config.json can’t be read. Fix it or delete it first.',
    'err.cloud.config_write_failed': 'Couldn’t write the cloud component’s settings',
    'err.cloud.pwsh_missing': 'PowerShell 7 (pwsh) wasn’t found; the cloud component needs it',
    'err.cloud.busy': 'The previous cloud action hasn’t finished yet',
    'err.cloud.timeout': 'The cloud component didn’t finish in time',
    'err.cloud.run_failed': 'The cloud component failed. Its log has the details.',
    'err.cloud.label_invalid': 'Names can only use letters, digits, - and _, up to 32 characters',
    'err.cloud.open_failed': 'Couldn’t open it',
    'err.update.localappdata_missing': 'Windows LOCALAPPDATA path unavailable',
    'err.update.launch_failed': 'The new version is in place but did not start; open it once by hand',
    'err.update.unsafe_data_location': 'Installation stopped — the data location could not be verified as safe after update. Quit Zebra, copy any in-bundle data folder to your user Application Support folder, fix ZEPPBRIDGE_DATA_DIR, and retry. Do not delete the old data.',
    'err.update.check_failed': "Couldn't check for a new version; it will try again next launch",
    'err.update.download_failed': "Couldn't download the new version (or its signature didn't match); it will try again next launch",
    'err.update.not_ready': "The new version hasn't finished downloading",
    'err.update.apply_failed': "Couldn't start the update; try again later",
    'err.update.package_invalid': 'The update package was not what we expected, so this update was skipped',
    'err.update.swap_failed': "Couldn't replace the program files; everything was put back as it was",
    'err.update.install_failed': "Couldn't install the new version",
    /* —— ai_tasks (P3 commands & attachment/grant errors) —— */
    'err.ai_task.invalid': "Task input invalid — check the fields",
    'err.ai_task.not_found': "Analysis task does not exist or was deleted",
    'err.ai_task.workout_not_found': "The selected workout does not exist locally",
    'err.ai_task.write_failed': "Could not write handoff files",
    'err.ai_task.attachment_missing': 'Attachment file no longer at its original location',
    'err.ai_handoff.file': 'Could not find the file to hand off',
    'err.ai_handoff.provider': 'That AI is not on the allow-list',
    'err.ai_handoff.unsupported': 'Copying files to the clipboard is not supported on this system yet',
    'err.ai_handoff.clipboard': 'Another app is using the clipboard — try again in a moment',
    'err.ai_template.invalid': "Template input invalid — check the fields",
    'err.ai_template.not_found': "Template does not exist or was deleted",
    'err.ai_template.builtin_readonly': 'Built-in templates are read-only — save as a user template',

    /* —— MCP access scope (the caller is a model; these are the human fallback) —— */
    'err.mcp.scope_denied': 'That request is outside the tasks shared with MCP',
    'err.mcp.scope_no_grants': 'No task is open to MCP yet — mark a task "open to MCP" on its page and retry',

    /* —— training plans —— */
    'err.training_plan.draft_not_found': 'This plan draft no longer exists',
    'err.training_plan.rest_unparsable': 'Cannot read the rest date, bedtime or sleep target',
    'err.training_plan.rest_outside_range': 'The rest date is outside the plan range',
    'err.training_plan.draft_closed': 'This plan draft was already sent or discarded',
    'err.training_plan.publish_not_found': 'No record of that push',
    'err.training_plan.rejected': 'Zepp did not accept this plan; the plan on your watch is unchanged',
    'err.training_plan.format': 'The training plan is malformed and was not saved',
    'err.training_plan.invalid': 'This plan did not pass validation and was not sent',
    'err.training_plan.ai_publish_disabled': 'AI is not allowed to publish training plans directly',
    'err.training_plan.needs_clear_confirmation': 'This would clear part of the plan on your watch; confirm it in Zebra',
    'err.coach_notes.empty': 'The note is empty',
    'err.coach_notes.too_long': 'A note can be at most 500 characters',
    'err.coach_notes.stale': 'This note was already handled or deleted',
    'err.coach_notes.export': 'Could not export the coach notes',
    'err.logbook.invalid': 'Something in this entry is not valid — check it and save again',
    'err.logbook.gear_taken': 'This Zepp shoe ID already belongs to another pair',
    'err.rings.save': 'Couldn’t save the rings image — check that your Pictures folder is writable',
  },
);

export const errorTextFor = (code: string | null | undefined): string | undefined => {
  if (!code) return undefined;
  return (messagesOf(messages) as Record<string, string>)[code];
};
