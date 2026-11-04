import { useId, useState } from 'react';
import { AowSelect } from '../../components/AowSelect';
import './language-settings.css';
import { getLanguage, languages, saveLanguage, t, type Language } from '../../lib/i18n';

export function LanguageSettings() {
  const [language, setLanguage] = useState<Language>(getLanguage);
  const id = useId();
  const [error, setError] = useState(false);
  const apply = () => {
    if (!window.confirm(t('language.reloadConfirmation'))) return;
    if (!saveLanguage(language)) { setError(true); return; }
    window.location.reload();
  };
  return <fieldset className="theme-picker">
    <legend>{t('language.title')}</legend>
    <div className="language-settings-control">
      <label htmlFor={id}>{t('language.label')}</label>
      <AowSelect id={id} label={t('language.label')} value={language} options={languages.map(item => ({ value: item.id, label: item.name }))}
        onChange={value => setLanguage(value as Language)} />
    </div>
    <p>{t('language.description')}</p>
    <button type="button" className="project-aow-dialog-button primary" disabled={language === getLanguage()} onClick={apply}>{t('language.apply')}</button>
    {error && <p role="alert">{t('language.saveFailed')}</p>}
  </fieldset>;
}
