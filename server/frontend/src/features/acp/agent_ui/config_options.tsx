import { t } from '../../../lib/i18n';
import { useEffect, useState } from 'react';
import { Check, ChevronDown, Star } from 'lucide-react';
import { applyEdits, modify, parse } from 'jsonc-parser';
import { acpApi } from '../api';
import { failure, object, list, text, type Data, type SessionSnapshot } from '../types';
import { Popover } from './popover';

interface Choice { value: string; name: string; description: string; group?: string }
function choices(option: Data): Choice[] {
  return list(option.options).flatMap(item => Array.isArray(item.options)
    ? list(item.options).map(value => ({ value: text(value.value), name: text(value.name), description: text(value.description), group: text(item.name) }))
    : [{ value: text(item.value), name: text(item.name), description: text(item.description) }]);
}
const matches = (value: string, query: string) => {
  let offset = 0;
  for (const character of query.toLocaleLowerCase()) { const next = value.toLocaleLowerCase().indexOf(character, offset); if (next < 0) return false; offset = next + 1; }
  return true;
};
function SelectOption({ option, favorites, disabled, onChange, onFavorite }: { option: Data; favorites: string[]; disabled: boolean; onChange: (value: string) => void; onFavorite?: (value: string) => void }) {
  const [query, setQuery] = useState('');
  const all = choices(option); const current = all.find(choice => choice.value === option.currentValue)?.name || t("acp.unknown_config_value");
  const filtered = all.filter(choice => matches(choice.name, query));
  const favoriteChoices = filtered.filter(choice => favorites.includes(choice.value));
  const entries: ({ label: string } | Choice)[] = [];
  if (favoriteChoices.length) {
    entries.push({ label: t("acp.favorites") }, ...favoriteChoices);
    if (!filtered[0]?.group) entries.push({ label: t("acp.all_options") });
  }
  let group: string | undefined;
  for (const choice of filtered) { if (choice.group !== group && choice.group) entries.push({ label: choice.group }); group = choice.group; entries.push(choice); }
  return <Popover label={text(option.name)} disabled={disabled} above trigger={<><span className="acp-option-value" title={text(option.description)}>{current}</span><ChevronDown size={12} /></>}>
    {close => <><div className="acp-menu-heading">{text(option.name)}</div>
      {all.length >= 5 && <input className="acp-menu-search" aria-label={t("acp.search", { value0: text(option.name) })} placeholder={t("acp.choose_an_option")} value={query} onChange={event => setQuery(event.target.value)} />}
      <div className="acp-menu-options">{entries.map((entry, index) => 'label' in entry ? <div className="acp-menu-heading" key={index}>{entry.label}</div>
        : <div className="acp-choice" key={`${entry.value}:${index}`}>
          <button type="button" role="menuitemradio" aria-checked={entry.value === option.currentValue} title={entry.description} onClick={() => { onChange(entry.value); setQuery(''); close(); }}><span>{entry.name}</span>{entry.value === option.currentValue && <Check size={14} />}</button>
          {onFavorite && <button type="button" className="acp-favorite" aria-label={`${favorites.includes(entry.value) ? t("common.remove_favorite") : t("acp.favorites")} ${entry.name}`} aria-pressed={favorites.includes(entry.value)} onClick={() => onFavorite(entry.value)}><Star size={13} fill={favorites.includes(entry.value) ? 'currentColor' : 'none'} /></button>}
        </div>)}{!filtered.length && <p className="acp-menu-heading">{t("acp.no_matching_options")}</p>}</div>
    </>}
  </Popover>;
}
export function ConfigOptions({ session, disabled, onAction }: { session: SessionSnapshot; disabled: boolean; onAction: (action: Data) => Promise<boolean> }) {
  const [favorites, setFavorites] = useState<Record<string, string[]>>({});
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  useEffect(() => {
    let cancelled = false;
    void acpApi.settings().then(file => { const agent = object(object(object(parse(file.content)).agent_servers)[session.agent_id]); if (!cancelled) setFavorites(object(agent.favorite_config_option_values) as Record<string, string[]>); }).catch(reason => { if (!cancelled) setError(failure(reason)); });
    return () => { cancelled = true; };
  }, [session.agent_id]);
  const preference = async (path: string[], value: unknown) => {
    const file = await acpApi.settings();
    const content = applyEdits(file.content, modify(file.content, ['agent_servers', session.agent_id, ...path], value, { formattingOptions: { insertSpaces: true, tabSize: 2 } }));
    await acpApi.saveSettings(content, file.revision);
  };
  const change = async (id: string, value: string | boolean, legacy = false) => {
    const option = session.config_options.find(item => (text(item.configId) || text(item.id)) === id);
    if (!legacy && (!option || (option.type === 'select' ? !choices(option).some(choice => choice.value === value) : option.type !== 'boolean'))) return;
    setSaving(true); setError('');
    try {
      try { await preference(legacy ? ['default_mode'] : ['default_config_options', id], value); } catch (reason) { setError(failure(reason)); }
      await onAction(legacy ? { action: 'set_mode', mode_id: value } : { action: 'set_config', config_id: id, value });
    } finally { setSaving(false); }
  };
  const toggleFavorite = async (id: string, value: string) => {
    const saved = Array.isArray(favorites[id]) ? favorites[id] : [];
    const next = saved.includes(value) ? saved.filter(item => item !== value) : [...saved, value];
    setSaving(true); setError('');
    try { await preference(['favorite_config_option_values', id], next); setFavorites(previous => ({ ...previous, [id]: next })); }
    catch (reason) { setError(failure(reason)); } finally { setSaving(false); }
  };
  const modern = session.config_options_supported || session.config_options.length > 0;
  const modes = list(session.modes?.availableModes);
  return <div className="acp-config-options" aria-label={t("acp.session_configuration")}>
    {!modern && !!modes.length && <SelectOption option={{ name: t("acp.mode"), currentValue: session.modes.currentModeId, options: modes.map(mode => ({ value: mode.id, name: mode.name, description: mode.description })) }} favorites={[]} disabled={disabled || saving} onChange={value => { void change('', value, true); }} />}
    {modern && session.config_options.map(option => {
      const id = text(option.configId) || text(option.id);
      if (option.type === 'select') return <SelectOption key={id} option={option} favorites={Array.isArray(favorites[id]) ? favorites[id] : []} disabled={disabled || saving} onChange={value => { void change(id, value); }} onFavorite={value => { void toggleFavorite(id, value); }} />;
      if (option.type === 'boolean') return <label key={id} className="acp-switch-label" title={text(option.description)}><span>{text(option.name)}</span><button type="button" role="switch" aria-label={text(option.name)} aria-checked={option.currentValue === true} disabled={disabled || saving} className="acp-switch" onClick={() => { void change(id, option.currentValue !== true); }}><span /></button></label>;
      return null;
    })}
    {error && <span className="acp-config-error" role="alert" title={error}>{error}</span>}
  </div>;
}
