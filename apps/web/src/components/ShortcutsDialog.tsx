import { Dialog } from './Dialog';
import { isMacPlatform } from '../lib/platform';

const SHORTCUTS: Array<{ keys: string[]; action: string }> = [
  { keys: [isMacPlatform ? '⌘' : 'Ctrl', 'K'], action: 'Open the command palette' },
  { keys: ['?'], action: 'Show keyboard shortcuts' },
  { keys: ['↑', '↓'], action: 'Move through palette results' },
  { keys: ['Enter'], action: 'Open the selected result' },
  { keys: ['Esc'], action: 'Close a dialog, drawer or palette' },
];

export function ShortcutsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  return (
    <Dialog open={open} onClose={onClose} title="Keyboard shortcuts" size="sm">
      <ul className="shortcut-list">
        {SHORTCUTS.map((shortcut) => (
          <li key={shortcut.action}>
            <span>{shortcut.action}</span>
            <span className="shortcut-keys">
              {shortcut.keys.map((key) => (
                <kbd key={key}>{key}</kbd>
              ))}
            </span>
          </li>
        ))}
      </ul>
    </Dialog>
  );
}
