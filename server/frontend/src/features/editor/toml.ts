import type { languages } from 'monaco-editor';

export const tomlConfiguration: languages.LanguageConfiguration = {
  comments: { lineComment: '#' },
  brackets: [['[', ']'], ['{', '}']],
  autoClosingPairs: [
    { open: '[', close: ']', notIn: ['string', 'comment'] },
    { open: '{', close: '}', notIn: ['string', 'comment'] },
    { open: '"', close: '"', notIn: ['string', 'comment'] },
    { open: "'", close: "'", notIn: ['string', 'comment'] },
  ],
  surroundingPairs: [
    { open: '[', close: ']' }, { open: '{', close: '}' },
    { open: '"', close: '"' }, { open: "'", close: "'" },
  ],
};

export const tomlLanguage: languages.IMonarchLanguage = {
  tokenPostfix: '.toml',
  key: /[A-Za-z0-9_-]+|"(?:[^"\\]|\\.)*"|'[^']*'/,
  escapes: /\\(?:[btnfr"\\]|u[0-9a-fA-F]{4}|U[0-9a-fA-F]{8})/,
  tokenizer: {
    root: [
      [/^[ \t]*\[\[?/, 'delimiter.square', '@table'],
      { include: '@keys' },
      { include: '@values' },
    ],
    keys: [
      [/(@key(?:[ \t]*\.[ \t]*@key)*)([ \t]*)(=)/, ['attribute.name', '', 'delimiter']],
    ],
    table: [
      [/\]\]?/, 'delimiter.square', '@pop'],
      [/@key[ \t]*$/, 'type.identifier', '@pop'],
      [/@key/, 'type.identifier'],
      [/[ \t]+$/, '', '@pop'],
      [/.$/, '', '@pop'],
      [/\./, 'delimiter'],
      [/[ \t]+/, ''],
    ],
    values: [
      [/[ \t]+/, ''],
      [/#.*$/, 'comment'],
      [/"""/, 'string', '@multilineBasicString'],
      [/'''/, 'string', '@multilineLiteralString'],
      [/"(?:[^"\\]|\\.)*$/, 'string.invalid'],
      [/'[^']*$/, 'string.invalid'],
      [/"/, 'string', '@basicString'],
      [/'/, 'string', '@literalString'],
      [/\b(?:true|false)\b/, 'keyword'],
      [/\d{4}-\d{2}-\d{2}(?:[Tt ]\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:[Zz]|[+-]\d{2}:\d{2})?)?|\d{2}:\d{2}:\d{2}(?:\.\d+)?/, 'number.date'],
      [/0x[0-9a-fA-F](?:_?[0-9a-fA-F])*|0o[0-7](?:_?[0-7])*|0b[01](?:_?[01])*/, 'number'],
      [/[+-]?(?:inf|nan)\b/, 'number'],
      [/[+-]?\d(?:_?\d)*(?:\.\d(?:_?\d)*)?(?:[eE][+-]?\d(?:_?\d)*)?\b/, 'number'],
      [/\[/, 'delimiter.square', '@array'],
      [/\{/, 'delimiter.curly', '@inlineTable'],
      [/[,.]/, 'delimiter'],
    ],
    array: [
      [/\]/, 'delimiter.square', '@pop'],
      { include: '@values' },
    ],
    inlineTable: [
      [/\}/, 'delimiter.curly', '@pop'],
      { include: '@keys' },
      { include: '@values' },
    ],
    basicString: [
      [/[^"\\]+/, 'string'],
      [/@escapes/, 'string.escape'],
      [/\\./, 'string.escape.invalid'],
      [/"/, 'string', '@pop'],
    ],
    literalString: [
      [/[^']+/, 'string'],
      [/'/, 'string', '@pop'],
    ],
    multilineBasicString: [
      [/"{3,5}/, 'string', '@pop'],
      [/@escapes/, 'string.escape'],
      [/\\[ \t]*$/, 'string.escape'],
      [/\\./, 'string.escape.invalid'],
      [/[^"\\]+/, 'string'],
      [/"/, 'string'],
    ],
    multilineLiteralString: [
      [/'{3,5}/, 'string', '@pop'],
      [/[^']+/, 'string'],
      [/'/, 'string'],
    ],
  },
};
