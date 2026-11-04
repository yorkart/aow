import '../../src/styles.css';
import { initializeTheme } from '../../src/lib/theme/store';
import { createRoot } from 'react-dom/client';
import { initializeLanguage, t } from '../../src/lib/i18n';
import { localizedFetch } from '../../src/lib/localizedFetch';
import { LanguageSettings } from '../../src/features/settings/LanguageSettings';

initializeLanguage();
initializeTheme();
createRoot(document.getElementById('root')!).render(<section className="project-aow-dialog" onKeyDown={event => { if (event.key === 'Escape') document.body.dataset.settingsEscape = 'true'; }}>
  <h1>{t('auth.sign_in_to_aow')}</h1>
  <LanguageSettings />
  <button type="button" onClick={() => { void localizedFetch('/api/i18n-probe'); }}>{t('acp.send')}</button>
</section>);
