import { t } from '../../lib/i18n';
import { useState, useSyncExternalStore } from 'react';
import { Check } from 'lucide-react';
import { useTheme } from '../../lib/theme/useTheme';
import { getDiffPalettePreference, resolveDiffPalette, setDiffPalettePreference, subscribeDiffPalette, type DiffPalettePreference } from '../../lib/theme/diffPalette';

const options: { id: DiffPalettePreference; name: string; description: string }[] = [
  { id: 'auto', name: t("settings.follow_theme"), description: t("settings.automatically_choose_a_diff_palette_for_the_interface_s") },
  { id: 'dark', name: t("settings.dark"), description: t("settings.use_the_original_monaco_dark_red_and_green_highlights") },
  { id: 'light', name: t("settings.light"), description: t("settings.red_and_green_highlights_for_light_editors") },
];

export function DiffPaletteSettings() {
  const theme = useTheme();
  const preference = useSyncExternalStore(subscribeDiffPalette, getDiffPalettePreference, getDiffPalettePreference);
  const [persistent, setPersistent] = useState(true);
  return <fieldset className="theme-picker diff-palette-picker">
    <legend>{t("settings.diff_palette")}</legend>
    <p className="diff-palette-description">{t("settings.an_explicit_selection_stays_when_the_interface_theme_changes_2")}</p>
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
      ? t("settings.the_diff_palette_is_saved_separately_in_this_browser")
      : t("settings.diff_palette_applied_but_the_browser_blocked_saving_refreshing")}</p>
  </fieldset>;
}
