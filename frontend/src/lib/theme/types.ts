/** Theme data is independent of React, Monaco and xterm. */
export interface Theme {
  id: string;
  name: string;
  family: 'AOW' | 'JetBrains' | 'VS Code';
  appearance: 'dark' | 'light';
  colors: {
    background: string;
    panel: string;
    sidebar: string;
    header: string;
    elevated: string;
    input: string;
    border: string;
    controlBorder: string;
    hover: string;
    selection: string;
    accent: string;
    primary: string;
    text: string;
    secondary: string;
    muted: string;
    disabled: string;
    link: string;
    success: string;
    error: string;
    warning: string;
    info: string;
    purple: string;
  };
  editor: {
    background: string;
    foreground: string;
    line: string;
    lineNumber: string;
    selection: string;
    cursor: string;
  };
  terminal: {
    background: string;
    foreground: string;
    cursor: string;
    selectionBackground: string;
    black: string;
    red: string;
    green: string;
    yellow: string;
    blue: string;
    magenta: string;
    cyan: string;
    white: string;
    brightBlack: string;
    brightRed: string;
    brightGreen: string;
    brightYellow: string;
    brightBlue: string;
    brightMagenta: string;
    brightCyan: string;
    brightWhite: string;
  };
  syntax: {
    comment: string;
    keyword: string;
    string: string;
    number: string;
    function: string;
    type: string;
    variable: string;
    operator: string;
  };
}
