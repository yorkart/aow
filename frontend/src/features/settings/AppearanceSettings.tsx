import { LanguageSettings } from './LanguageSettings';
import { t } from '../../lib/i18n';
import { useState, useSyncExternalStore, type CSSProperties } from 'react';
import { Check } from 'lucide-react';
import { themes } from '../../lib/theme/themes';
import { setTheme } from '../../lib/theme/store';
import { useTheme } from '../../lib/theme/useTheme';
import type { Theme } from '../../lib/theme/types';
import { getTerminalPalettePreference, resolveTerminalPalette, setTerminalPalettePreference, subscribeTerminalPalette, type TerminalPalettePreference } from '../../lib/theme/terminalPalette';
import { DiffPaletteSettings } from './DiffPaletteSettings';
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
    { id: 'auto', name: t("settings.follow_theme"), description: t("settings.automatically_choose_a_soft_palette_for_the_interface_s") },
    { id: 'soft-dark', name: t("settings.soft_dark"), description: t("settings.dark_background_with_the_original_terminal_palette") },
    { id: 'soft-light', name: t("settings.soft_light"), description: t("settings.light_background_with_soft_readable_text_colors") },
  ];
  return <div className="project-aow-dialog-body appearance-settings">
    <div className="project-aow-settings-heading"><div><h2>{t("navigation.appearance")}</h2><p>{t("settings.choose_a_workspace_theme_terminal_and_diff_palettes_can")}</p></div></div>
    <LanguageSettings />
    <fieldset className="theme-picker color-theme-picker">
      <legend>{t("settings.color_theme")}</legend>
      <div className="theme-grid">
        {themes.map(item => <label className="theme-card" key={item.id} style={previewColors(item)}>
          <input type="radio" name="aow-theme" value={item.id} checked={item.id === theme.id}
            onChange={() => setPersistent(setTheme(item.id))} />
          <span className="theme-preview" aria-hidden="true">
            <span className="theme-preview-bar"><i /><i /><i /></span>
            <span className="theme-preview-sidebar"><b /><b /><b /><b /></span>
            <span className="theme-preview-editor"><span><em>const</em> theme = {'{'}</span><span>  name: <strong>"AoW"</strong>,</span><span>  ready: <em>true</em></span><span>{'}'}</span><small>❯ npm run dev</small></span>
          </span>
          <span className="theme-card-caption"><span><strong>{item.name}</strong><small>{item.family} · {item.appearance === 'dark' ? t("settings.dark") : t("settings.light")}</small></span>
            <span className="theme-card-check" aria-hidden="true">{item.id === theme.id && <Check />}</span>
          </span>
        </label>)}
      </div>
    </fieldset>
    <p className="theme-selection-status" role="status">{persistent
      ? t("settings.current_theme_your_selection_is_saved_in_this_browser", { value0: theme.name })
      : t("settings.applied_but_the_browser_blocked_saving_refreshing_may_restore", { value0: theme.name })}</p>
    <fieldset className="theme-picker terminal-palette-picker">
      <legend>{t("settings.terminal_palette")}</legend>
      <p className="terminal-palette-description">{t("settings.an_explicit_selection_stays_when_the_interface_theme_changes")}</p>
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
        ? t("settings.the_terminal_palette_is_saved_separately_in_this_browser")
        : t("settings.terminal_palette_applied_but_the_browser_blocked_saving_refreshing")}</p>
    </fieldset>
    <DiffPaletteSettings />
  </div>;
}
