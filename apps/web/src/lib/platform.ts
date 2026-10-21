export const isMacPlatform =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform);

export const commandKeyLabel = isMacPlatform ? '⌘K' : 'Ctrl K';
