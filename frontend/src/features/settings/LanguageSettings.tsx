import { useState } from 'react';
import { getLanguage, languages, saveLanguage, t, type Language } from '../../lib/i18n';

export function LanguageSettings() {
  const [language, setLanguage] = useState<Language>(getLanguage);
  const [error, setError] = useState(false);
  const apply = () => {
    if (!window.confirm(t('language.reloadConfirmation'))) return;
    if (!saveLanguage(language)) { setError(true); return; }
    window.location.reload();
  };
  return <fieldset className="theme-picker">
    <legend>{t('language.title')}</legend>
    <label>{t('language.label')}{' '}
      <select value={language} onChange={event => setLanguage(event.target.value as Language)}>
        {languages.map(item => <option key={item.id} value={item.id}>{item.name}</option>)}
      </select>
    </label>
    <p>{t('language.description')}</p>
    <button type="button" className="project-aow-dialog-button primary" disabled={language === getLanguage()} onClick={apply}>{t('language.apply')}</button>
    {error && <p role="alert">{t('language.saveFailed')}</p>}
  </fieldset>;
}
