import { createRoot } from 'react-dom/client';
import { useRef, useState } from 'react';
import { AppearanceSettings } from '../src/features/settings/AppearanceSettings';
import MonacoEditor, { DiffEditor } from '../src/features/editor/MonacoEditor';
import { TerminalPaneView } from '../src/features/terminals/TerminalPaneView';
import { MarkdownContent } from '../src/components/MarkdownContent';
import { initializeTheme } from '../src/lib/theme/store';
import { useTheme } from '../src/lib/theme/useTheme';
import type { TerminalPaneControls } from '../src/features/terminals/terminalPresentation';
import '../src/styles.css';
import '../src/components/conversation-theme.css';
import '../src/components/conversation/conversation-layout.css';
import './theme-preview.css';

initializeTheme();
const noop = () => {};
const initialCode = '// Theme changes keep your edits\nexport const greeting = "Hello, AoW";\nconst ready = true;\nconst version = 1;\n';
const pane = { id: 'theme-pane', name: 'Theme terminal', cwd: '/preview', shell: '/bin/sh', status: 'running' as const };
const diagram = '### Conversation preview\n\n主题切换后，对话、代码和图表保持一致。\n\n> 保留正在进行的工作。\n\n```mermaid\nflowchart LR\n  A[Choose theme] --> B[Keep working]\n```';

function Preview() {
  const [code, setCode] = useState(initialCode);
  const theme = useTheme();
  const controls = useRef<TerminalPaneControls | null>(null);
  return <main className="theme-workbench" aria-label="Theme integration preview">
    <aside className="theme-preview-settings"><AppearanceSettings /></aside>
    <section className="theme-preview-surfaces">
      <header><strong>{theme.name}</strong><span>Editor · Diff · Terminal · Conversation</span></header>
      <div className="theme-preview-code">
        <MonacoEditor language="typescript" value={code} onChange={value => setCode(value ?? '')}
          onMount={editor => Object.assign(window, { themeEditor: editor })}
          options={{ minimap: { enabled: false }, automaticLayout: true, fontSize: 12 }} />
        <DiffEditor language="typescript" original={initialCode} modified={initialCode.replace('version = 1', 'version = 2')}
          onMount={editor => Object.assign(window, { themeDiff: editor })}
          options={{ readOnly: true, minimap: { enabled: false }, automaticLayout: true, renderSideBySide: false, fontSize: 12 }} />
      </div>
      <div className="theme-preview-terminal">
        <TerminalPaneView renderer="dom" visible active tabId="theme-tab" pane={pane} controlsRef={controls}
          onFocus={noop} onStatus={noop} onFrameChange={() => Object.assign(window, { themeTerminalControls: controls.current })} />
      </div>
      <section className="theme-preview-conversation aow-conversation-theme">
        <MarkdownContent text={diagram} className="project-aow-snapshot-markdown conversation-prose" />
      </section>
    </section>
  </main>;
}

createRoot(document.getElementById('root')!).render(<Preview />);
