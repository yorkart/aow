import { t } from '../../lib/i18n';
import type { AutomationRun } from './types';
import './run-parameters.css';

export function RunParameters({ run }: { run: AutomationRun }) {
  if (run.source !== 'manual') return null;
  const entries = Object.entries(run.variables ?? {});
  return <section className="automation-run-parameters" aria-label={t("automations.trigger_arguments")}>
    <h3>{t("automations.trigger_arguments")}</h3>
    {run.variables == null ? <p>{t("automations.trigger_arguments_were_not_saved_for_this_run")}</p> : entries.length === 0 ? <p>{t("automations.this_run_has_no_arguments")}</p>
      : <dl className="automation-run-parameter-list">{entries.map(([name, value]) => <div key={name}><dt>{name}</dt><dd><pre>{value}</pre></dd></div>)}</dl>}
  </section>;
}
