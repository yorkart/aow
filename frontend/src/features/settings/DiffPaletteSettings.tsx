import { useState, useSyncExternalStore } from 'react';
import { Check } from 'lucide-react';
import { useTheme } from '../../lib/theme/useTheme';
import { getDiffPalettePreference, resolveDiffPalette, setDiffPalettePreference, subscribeDiffPalette, type DiffPalettePreference } from '../../lib/theme/diffPalette';

const options: { id: DiffPalettePreference; name: string; description: string }[] = [
  { id: 'auto', name: '跟随主题', description: '按界面深浅模式自动选择差异配色' },
  { id: 'dark', name: '深色', description: '恢复原有 Monaco 深色红绿标记' },
  { id: 'light', name: '浅色', description: '适合浅色编辑器的红绿标记' },
];

export function DiffPaletteSettings() {
  const theme = useTheme();
  const preference = useSyncExternalStore(subscribeDiffPalette, getDiffPalettePreference, getDiffPalettePreference);
  const [persistent, setPersistent] = useState(true);
  return <fieldset className="theme-picker diff-palette-picker">
    <legend>Diff 配色</legend>
    <p className="diff-palette-description">单独选择后不随界面主题变化。仅调整差异标记，代码文字继续使用编辑器语法配色。</p>
    <div className="theme-grid">
      {options.map(item => {
        const palette = resolveDiffPalette(theme.appearance, item.id);
        const appearance = item.id === 'auto' ? theme.appearance : item.id;
        return <label className="theme-card" key={item.id}>
          <input type="radio" name="aow-diff-palette" value={item.id} checked={item.id === preference}
            onChange={() => setPersistent(setDiffPalettePreference(item.id))} />
          <span className="diff-palette-preview" aria-hidden="true" style={{ background: appearance === 'dark' ? '#1e1e1e' : '#ffffff', color: appearance === 'dark' ? '#cccccc' : '#383a42' }}>
            <span style={{ background: palette.removedLine }}>− const value = <b style={{ background: palette.removedText }}>before</b>;</span>
            <span style={{ background: palette.insertedLine }}>+ const value = <b style={{ background: palette.insertedText }}>after</b>;</span>
          </span>
          <span className="theme-card-caption"><span><strong>{item.name}</strong><small>{item.description}</small></span>
            <span className="theme-card-check" aria-hidden="true">{item.id === preference && <Check />}</span>
          </span>
        </label>;
      })}
    </div>
    <p className="theme-selection-status" role="status">{persistent
      ? 'Diff 配色选择会单独保存在此浏览器中，已有对比即时更新。'
      : '已应用 Diff 配色，但浏览器阻止了保存；刷新后可能恢复跟随主题。'}</p>
  </fieldset>;
}
