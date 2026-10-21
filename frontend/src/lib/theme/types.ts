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
