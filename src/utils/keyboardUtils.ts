export interface KeybindDefinition {
  action: string;
  description: string;
  defaultCombo: string[];
  macDefaultCombo?: string[];
  section: 'view' | 'panels' | 'editing';
}

export interface KeybindSection {
  id: KeybindDefinition['section'];
  label: string;
}

export const KEYBIND_SECTIONS: KeybindSection[] = [
  { id: 'view', label: 'settings.keybinds.sections.view' },
  { id: 'panels', label: 'settings.keybinds.sections.panels' },
  { id: 'editing', label: 'settings.keybinds.sections.editing' },
];

export const KEYBIND_DEFINITIONS: KeybindDefinition[] = [
  {
    action: 'copy_image_path',
    description: 'settings.keybinds.actions.copy_image_path',
    defaultCombo: ['ctrl', 'KeyL'],
    section: 'view',
  },
  {
    action: 'open_new_image',
    description: 'settings.keybinds.actions.open_new_image',
    defaultCombo: ['ctrl', 'KeyO'],
    section: 'view',
  },
  {
    action: 'zoom_in_step',
    description: 'settings.keybinds.actions.zoom_in_step',
    defaultCombo: ['ArrowUp'],
    section: 'view',
  },
  {
    action: 'zoom_out_step',
    description: 'settings.keybinds.actions.zoom_out_step',
    defaultCombo: ['ArrowDown'],
    section: 'view',
  },
  {
    action: 'cycle_zoom',
    description: 'settings.keybinds.actions.cycle_zoom',
    defaultCombo: ['Space'],
    section: 'view',
  },
  {
    action: 'zoom_in',
    description: 'settings.keybinds.actions.zoom_in',
    defaultCombo: ['ctrl', 'Equal'],
    section: 'view',
  },
  {
    action: 'zoom_out',
    description: 'settings.keybinds.actions.zoom_out',
    defaultCombo: ['ctrl', 'Minus'],
    section: 'view',
  },
  {
    action: 'zoom_fit',
    description: 'settings.keybinds.actions.zoom_fit',
    defaultCombo: ['ctrl', 'Digit0'],
    section: 'view',
  },
  {
    action: 'zoom_100',
    description: 'settings.keybinds.actions.zoom_100',
    defaultCombo: ['ctrl', 'Digit1'],
    section: 'view',
  },
  {
    action: 'toggle_fullscreen',
    description: 'settings.keybinds.actions.toggle_fullscreen',
    defaultCombo: ['KeyF'],
    section: 'view',
  },
  {
    action: 'show_original',
    description: 'settings.keybinds.actions.show_original',
    defaultCombo: ['KeyB'],
    section: 'view',
  },
  {
    action: 'toggle_adjustments',
    description: 'settings.keybinds.actions.toggle_adjustments',
    defaultCombo: ['KeyD'],
    section: 'panels',
  },
  {
    action: 'toggle_crop_panel',
    description: 'settings.keybinds.actions.toggle_crop_panel',
    defaultCombo: ['KeyR'],
    section: 'panels',
  },
  {
    action: 'toggle_masks',
    description: 'settings.keybinds.actions.toggle_masks',
    defaultCombo: ['KeyM'],
    section: 'panels',
  },
  {
    action: 'toggle_ai',
    description: 'settings.keybinds.actions.toggle_ai',
    defaultCombo: ['KeyK'],
    section: 'panels',
  },
  {
    action: 'toggle_analytics',
    description: 'settings.keybinds.actions.toggle_analytics',
    defaultCombo: ['KeyA'],
    section: 'panels',
  },
  {
    action: 'toggle_export',
    description: 'settings.keybinds.actions.toggle_export',
    defaultCombo: ['KeyE'],
    section: 'panels',
  },
  {
    action: 'toggle_left_panel',
    description: 'settings.keybinds.actions.toggle_left_panel',
    defaultCombo: ['ctrl', 'shift', 'KeyB'],
    section: 'panels',
  },
  {
    action: 'toggle_right_panel',
    description: 'settings.keybinds.actions.toggle_right_panel',
    defaultCombo: ['ctrl', 'KeyB'],
    section: 'panels',
  },
  {
    action: 'open_settings',
    description: 'settings.keybinds.actions.open_settings',
    defaultCombo: ['ctrl', 'Comma'],
    section: 'view',
  },
  { action: 'undo', description: 'settings.keybinds.actions.undo', defaultCombo: ['ctrl', 'KeyZ'], section: 'editing' },
  { action: 'redo', description: 'settings.keybinds.actions.redo', defaultCombo: ['ctrl', 'KeyY'], section: 'editing' },
  {
    action: 'copy_adjustments',
    description: 'settings.keybinds.actions.copy_adjustments',
    defaultCombo: ['ctrl', 'KeyC'],
    section: 'editing',
  },
  {
    action: 'paste_adjustments',
    description: 'settings.keybinds.actions.paste_adjustments',
    defaultCombo: ['ctrl', 'KeyV'],
    section: 'editing',
  },
  {
    action: 'rotate_left',
    description: 'settings.keybinds.actions.rotate_left',
    defaultCombo: ['BracketLeft'],
    section: 'editing',
  },
  {
    action: 'rotate_right',
    description: 'settings.keybinds.actions.rotate_right',
    defaultCombo: ['BracketRight'],
    section: 'editing',
  },
  {
    action: 'toggle_crop',
    description: 'settings.keybinds.actions.toggle_crop',
    defaultCombo: ['KeyS'],
    section: 'editing',
  },
  {
    action: 'auto_straighten',
    description: 'settings.keybinds.actions.auto_straighten',
    defaultCombo: ['shift', 'KeyS'],
    section: 'editing',
  },
  {
    action: 'brush_size_up',
    description: 'settings.keybinds.actions.brush_size_up',
    defaultCombo: ['ctrl', 'ArrowUp'],
    section: 'editing',
  },
  {
    action: 'brush_size_down',
    description: 'settings.keybinds.actions.brush_size_down',
    defaultCombo: ['ctrl', 'ArrowDown'],
    section: 'editing',
  },
];

const symMap: Record<string, string> = {
  Space: 'Space',
  Backspace: '⌫',
  Enter: 'Enter',
  Delete: 'Delete',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  BracketLeft: '[',
  BracketRight: ']',
  Minus: '-',
  Equal: '+',
  Comma: ',',
  Period: '.',
  Slash: '/',
  Semicolon: ';',
  Quote: "'",
  Backquote: '`',
  Backslash: '\\',
  Tab: 'Tab',
  Escape: 'Esc',
  PageUp: 'Page Up',
  PageDown: 'Page Down',
  Home: 'Home',
  End: 'End',
  Insert: 'Insert',
  NumpadAdd: 'Numpad +',
  NumpadMultiply: 'Numpad *',
  NumpadDivide: 'Numpad /',
  NumpadSubtract: 'Numpad -',
  NumpadDecimal: 'Numpad .',
  NumpadComma: 'Numpad ,',
  NumpadEnter: 'Numpad Enter',
  NumpadEqual: 'Numpad =',
  CapsLock: 'Caps Lock',
  PrintScreen: 'PrtSc',
};

export function getDefaultCombo(def: KeybindDefinition, osPlatform?: string): string[] {
  return (osPlatform === 'macos' && def.macDefaultCombo) || def.defaultCombo;
}

// Canonical identifiers keep stored shortcuts compatible while the symbol
// comes from the active keyboard layout rather than a physical QWERTY key.
const punctuationCodes: Record<string, string> = {
  '[': 'BracketLeft',
  ']': 'BracketRight',
  '-': 'Minus',
  '=': 'Equal',
  ',': 'Comma',
  '.': 'Period',
  '/': 'Slash',
  ';': 'Semicolon',
  "'": 'Quote',
  '`': 'Backquote',
  '\\': 'Backslash',
};

export function normalizeCombo(event: KeyboardEvent, osPlatform?: string): string[] {
  if (event.isComposing || event.key === 'Dead' || event.key === 'Process') return [];
  const isMacDelete = osPlatform === 'macos' && event.code === 'Backspace' && (event.ctrlKey || event.metaKey);
  const parts: string[] = [];
  if ((event.ctrlKey || event.metaKey) && !isMacDelete) parts.push('ctrl');
  if (event.shiftKey) parts.push('shift');
  if (event.altKey) parts.push('alt');
  let code = isMacDelete ? 'Delete' : event.code;
  if (event.key && /^[a-zA-Z]$/.test(event.key)) {
    code = `Key${event.key.toUpperCase()}`;
  } else if (/^Numpad[0-9]$/.test(code)) {
    code = `Digit${code.slice(-1)}`;
  } else if (code === 'NumpadAdd') {
    code = 'Equal';
  } else if (code === 'NumpadSubtract') {
    code = 'Minus';
  } else if (
    !code.startsWith('Numpad') &&
    !(/^Digit[0-9]$/.test(code) && !event.ctrlKey && !event.metaKey && !event.altKey) &&
    punctuationCodes[event.key]
  ) {
    code = punctuationCodes[event.key];
  }
  if (isValidShortcutKey(code)) {
    parts.push(code);
  }
  return parts;
}

function codeToDisplayLabel(code: string): string | null {
  if (/^Key[A-Z]$/.test(code) || /^Digit[0-9]$/.test(code)) {
    return code[code.length - 1].toUpperCase();
  }
  if (/^Numpad[0-9]$/.test(code)) {
    return `Numpad ${code.slice(-1)}`;
  }
  return symMap[code] ?? null;
}

function isValidShortcutKey(code: string): boolean {
  if (code.startsWith('Key') || code.startsWith('Digit')) return true;
  if (code.startsWith('F') && /^\d+$/.test(code.slice(1))) return true;
  if (/^Numpad[0-9]$/.test(code)) return true;
  return code in symMap;
}

export function formatKeyCode(key: string, osPlatform: string): string {
  if (key === 'ctrl') return osPlatform === 'macos' ? '⌘' : 'Ctrl';
  if (key === 'shift') return 'Shift';
  if (key === 'alt') return osPlatform === 'macos' ? '⌥' : 'Alt';
  if (key === 'Delete' && osPlatform === 'macos') return 'Delete / ⌘+⌫';
  const label = codeToDisplayLabel(key);
  return label || key;
}
