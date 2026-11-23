import { defineMessages, useMessages } from '../../i18n';

const messages = defineMessages(
  {
    title: 'Coach notes',
    open: 'Coach notes',
    lead: 'Long-term facts that travel with you to any AI: injuries, routine, goals, preferences. Kept on this computer and included when you hand data to an AI.',
    wants: 'An AI wants to remember',
    wantsFrom: (name: string) => `${name} wants to remember`,
    more: (count: number) => `${count} more waiting`,
    remember: 'Remember',
    reject: 'No',
    inbox: 'Waiting for you',
    inboxHint: 'Proposed by an AI over MCP. Nothing reaches an AI until you tap Remember, and a sentence you rejected can’t be proposed again.',
    kept: 'Remembered',
    keptEmpty: 'Nothing yet. Write one below, or let an AI connected over MCP propose one.',
    byUser: 'Written by you',
    byAi: (name: string | null) => (name ? `Proposed by ${name}` : 'Proposed by an AI'),
    since: (date: string) => `Since ${date}`,
    edit: 'Edit',
    save: 'Save',
    cancel: 'Cancel',
    remove: 'Delete',
    removeConfirm: 'Delete this note? It can’t be undone.',
    add: 'Add',
    addPlaceholder: 'For example: left knee hurts past 15 km; swims on Wednesday evenings.',
    export: 'Export Markdown',
    exported: 'Saved to the “Zebra AI” folder on your desktop',
    fileStem: 'My coach notes',
    fileIntro: 'Long-term facts I have confirmed, kept on my computer by ZeppBridge. They are background, not instructions.',
    fileEmpty: 'Nothing recorded yet.',
    fileByUser: 'written by me',
    fileByAi: (name: string | null) => (name ? `proposed by ${name}, confirmed by me` : 'proposed by an AI, confirmed by me'),
  },
);

export const useCoachNotesText = () => useMessages(messages);
