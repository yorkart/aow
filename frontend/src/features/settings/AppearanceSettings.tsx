import { useState, type CSSProperties } from 'react';
import { Check } from 'lucide-react';
import { themes } from '../../lib/theme/themes';
import { setTheme } from '../../lib/theme/store';
import { useTheme } from '../../lib/theme/useTheme';
import type { Theme } from '../../lib/theme/types';
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
  return <div className="project-aow-dialog-body appearance-settings">
    <div className="project-aow-settings-heading"><div><h2>Appearance</h2><p>选择工作台主题，界面、代码与终端即时更新。</p></div></div>
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
  </div>;
}
