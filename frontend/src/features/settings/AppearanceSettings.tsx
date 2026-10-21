import { useState, useSyncExternalStore, type CSSProperties } from 'react';
import { Check } from 'lucide-react';
import { themes } from '../../lib/theme/themes';
import { setTheme } from '../../lib/theme/store';
import { useTheme } from '../../lib/theme/useTheme';
import type { Theme } from '../../lib/theme/types';
import { getTerminalPalettePreference, resolveTerminalPalette, setTerminalPalettePreference, subscribeTerminalPalette, type TerminalPalettePreference } from '../../lib/theme/terminalPalette';
import './appearance-settings.css';

function previewColors(theme: Theme): CSSProperties {
  return {
    '--preview-background': theme.editor.background,
    '--preview-sidebar': theme.colors.sidebar,
    '--preview-header': theme.colors.header,
    '--preview-border': theme.colors.border,
    '--preview-text': theme.colors.text,
    '--preview-muted': theme.colors.secondary,
    '--preview-accent': theme.colors.accent,
    '--preview-keyword': theme.syntax.keyword,
    '--preview-string': theme.syntax.string,
  } as CSSProperties;
}

export function AppearanceSettings() {
  const theme = useTheme();
  const [persistent, setPersistent] = useState(true);
  const [terminalPersistent, setTerminalPersistent] = useState(true);
  const terminalPreference = useSyncExternalStore(subscribeTerminalPalette, getTerminalPalettePreference, getTerminalPalettePreference);
  const terminalOptions: { id: TerminalPalettePreference; name: string; description: string }[] = [
    { id: 'auto', name: '跟随主题', description: '按界面深浅模式自动选择柔和配色' },
    { id: 'soft-dark', name: '柔和深色', description: '深色背景，保留原有终端配色' },
    { id: 'soft-light', name: '柔和浅色', description: '浅色背景，柔和且清晰的文字颜色' },
  ];
  return <div className="project-aow-dialog-body appearance-settings">
    <div className="project-aow-settings-heading"><div><h2>Appearance</h2><p>选择工作台主题，终端配色可独立设置。</p></div></div>
    <fieldset className="theme-picker">
      <legend>颜色主题</legend>
      <div className="theme-grid">
        {themes.map(item => <label className="theme-card" key={item.id} style={previewColors(item)}>
          <input type="radio" name="aow-theme" value={item.id} checked={item.id === theme.id}
            onChange={() => setPersistent(setTheme(item.id))} />
          <span className="theme-preview" aria-hidden="true">
            <span className="theme-preview-bar"><i /><i /><i /></span>
            <span className="theme-preview-sidebar"><b /><b /><b /><b /></span>
            <span className="theme-preview-editor"><span><em>const</em> theme = {'{'}</span><span>  name: <strong>"AoW"</strong>,</span><span>  ready: <em>true</em></span><span>{'}'}</span><small>❯ npm run dev</small></span>
          </span>
          <span className="theme-card-caption"><span><strong>{item.name}</strong><small>{item.family} · {item.appearance === 'dark' ? '深色' : '浅色'}</small></span>
            <span className="theme-card-check" aria-hidden="true">{item.id === theme.id && <Check />}</span>
          </span>
        </label>)}
      </div>
    </fieldset>
    <p className="theme-selection-status" role="status">{persistent
      ? `当前主题：${theme.name}。选择会保存在此浏览器中。`
      : `已应用 ${theme.name}，但浏览器阻止了保存；刷新后可能恢复默认主题。`}</p>
    <fieldset className="theme-picker terminal-palette-picker">
      <legend>终端配色</legend>
      <p className="terminal-palette-description">单独选择后，切换界面主题会保留终端配色。已有终端即时更新，无需重连。</p>
      <div className="theme-grid">
        {terminalOptions.map(item => {
          const palette = resolveTerminalPalette(theme.appearance, item.id);
          return <label className="theme-card" key={item.id}>
            <input type="radio" name="aow-terminal-palette" value={item.id} checked={item.id === terminalPreference}
              onChange={() => setTerminalPersistent(setTerminalPalettePreference(item.id))} />
            <span className="terminal-palette-preview" aria-hidden="true" style={{ background: palette.background, color: palette.foreground }}>
              <span><b style={{ color: palette.brightGreen }}>user@host</b>:<b style={{ color: palette.brightBlue }}>~/workspace</b>$ ls</span>
              <span><b style={{ color: palette.brightBlue }}>src/  docs/</b>  <b style={{ color: palette.brightYellow }}>README.md</b></span>
              <span>{[palette.red, palette.green, palette.yellow, palette.blue, palette.magenta, palette.cyan].map((color, index) => <i key={index} style={{ background: color }} />)}</span>
            </span>
            <span className="theme-card-caption"><span><strong>{item.name}</strong><small>{item.description}</small></span>
              <span className="theme-card-check" aria-hidden="true">{item.id === terminalPreference && <Check />}</span>
            </span>
          </label>;
        })}
      </div>
      <p className="theme-selection-status" role="status">{terminalPersistent
        ? '终端配色选择会单独保存在此浏览器中。'
        : '已应用终端配色，但浏览器阻止了保存；刷新后可能恢复跟随主题。'}</p>
    </fieldset>
  </div>;
}
