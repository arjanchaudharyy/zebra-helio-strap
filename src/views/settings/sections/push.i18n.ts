import { defineMessages } from '../../../i18n';

export const pushMessages = defineMessages(
  {
    secTitle: 'Push to your own site',
    lead: 'After every sync, your data is encrypted and sent to your own website, so Personal Agents like Muse and Hark — and your phone — can read it any time. The site only ever holds ciphertext; nothing goes to Zebra or anyone else.',
    flowComputer: 'This computer',
    flowLock: 'Encrypt',
    flowSite: 'Your site',
    flowReader: 'Phone / AI',
    companionTitle: 'Zebra Cloud is set up on this computer',
    companionGo: 'Take it over',
    companionDone: 'Taken over. Sending the first round…',
    pathsTitle: 'How to start',
    haveSite: 'I already have a site',
    fill: 'Fill in',
    noSite: "I don't have a site yet",
    siteLabel: 'Site address',
    sitePlaceholder: 'example.com',
    siteHint: 'A bare domain becomes https://health.your-domain',
    codeLabel: 'Connection code',
    codePlaceholder: '{"tenant": "…", "dk": "…", "writeKey": "zbw_…"}',
    codeHint: 'The one line you got when the site was set up. Paste all of it. It goes into the system credential store and is never shown again.',
    codeKeep: 'Leave empty to keep the connection code already saved',
    save: 'Save and push once',
    saving: 'Saving…',
    saved: 'Saved',
    cancel: 'Cancel',
    askTitle: 'Let your local AI set one up',
    askSub: 'Copy this message and paste it into an AI that can use your computer, such as Claude Code or Codex. It walks you through every step and ends by giving you a site address and a connection code to fill in here.',
    askNeeds: 'You need a Cloudflare account and a domain of your own. About 20 minutes.',
    copyPrompt: 'Copy the message for your AI',
    copied: 'Copied. Paste it into your AI',
    copyFailed: "Couldn't copy. Select the text below by hand",
    showPrompt: 'Read the message',
    aiPrompt: (language: string) => `You are the assistant on my computer. I use Zebra (an app that keeps my Zepp / Amazfit health data on this computer) and I want it to push that data, encrypted, to my own website after every sync, so my phone and my Personal Agent can read it. Please walk me through setting up the site, one step at a time.

How to work with me:
- Before each step, say in a sentence or two what you are about to do, and wait for my OK. Stop whenever I need to sign in, pay or confirm something, and let me do it myself.
- Talk to me in ${language}. I am not a programmer, so keep jargon to a minimum.

What we need to end up with:
1. A site deployed from the Zebra Cloud site template (the worker/ folder of the ZeppBridge-Cloud repository, a Cloudflare Worker) at https://health.<my domain>. This needs a Cloudflare account and a domain on Cloudflare; if I have neither, first tell me how to sign up and roughly what it costs.
2. Run node uploader/keys.mjs init in the ZeppBridge-Cloud folder. It prints one line of JSON containing tenant, dk, writeKey and readKey.
3. At the end, give me exactly two things, pasted as they are so I can copy them into Zebra:
   - the site address (health.<my domain>)
   - the connection code: the whole JSON line from step 2
   Separately, tell me that readKey is a read-only key I can later give to my phone or Personal Agent.

Rules:
- The connection code and the keys are passwords: never write them into files, never commit them to GitHub, never send them to anyone.
- Do not change any of Zebra's own files or its database.
- If you cannot find the Zebra Cloud site template, tell me so. Do not write a replacement yourself: the encryption format has to match Zebra exactly.`,
    promptLanguage: 'English',
    statusOn: 'On',
    statusOff: 'Paused',
    statusNoKey: 'Connection code missing, nothing can be sent',
    statusBroken: "The push settings can't be read",
    never: 'Not pushed yet',
    lastOk: (when: string, count: number) => `${when}: pushed ${count} ${count === 1 ? 'item' : 'items'}`,
    lastUnchanged: (when: string) => `${when}: checked, nothing new`,
    lastLocked: (when: string) => `${when}: the previous round was still running, skipped`,
    lastFailed: (when: string) => `${when}: push failed`,
    pushNow: 'Push now',
    pushing: 'Pushing…',
    change: 'Change site or code',
    remove: 'Remove',
    removeConfirm: 'Click again to remove (data on the site is kept)',
    removed: 'Removed',
    tasksTitle: "Zebra Cloud's scheduled tasks are still there",
    tasksSub: 'Zebra pushes by itself now, so those two scheduled tasks would only upload everything a second time. Copy the command and run it in PowerShell to delete them.',
    copyTasks: 'Copy the delete command',
    tasksCopied: 'Copied. Run it in PowerShell',
    failed: "That didn't work. Try again",
  },
);
