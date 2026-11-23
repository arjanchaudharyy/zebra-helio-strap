export const isMac = () => /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);
export const isWindows = () => /Win/i.test(navigator.platform || navigator.userAgent);

export const withPlatformKeys = (text: string) => (isMac() ? text.replace(/Ctrl\+V/g, '⌘V') : text);
