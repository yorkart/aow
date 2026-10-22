import { createRoot } from 'react-dom/client';
import { initializeLanguage, t } from '../../src/lib/i18n';
import { localizedFetch } from '../../src/lib/localizedFetch';
import { LanguageSettings } from '../../src/features/settings/LanguageSettings';

initializeLanguage();
createRoot(document.getElementById('root')!).render(<>
  <h1>{t('auth.sign_in_to_aow')}</h1>
  <LanguageSettings />
  <button type="button" onClick={() => { void localizedFetch('/api/i18n-probe'); }}>{t('acp.send')}</button>
</>);
