import type { AutomationRun } from './types';
import './run-parameters.css';

export function RunParameters({ run }: { run: AutomationRun }) {
  if (run.source !== 'manual') return null;
  const entries = Object.entries(run.variables ?? {});
  return <section className="automation-run-parameters" aria-label="触发参数">
    <h3>触发参数</h3>
    {run.variables == null ? <p>该执行记录未保存触发参数。</p> : entries.length === 0 ? <p>本次执行无参数。</p>
      : <dl className="automation-run-parameter-list">{entries.map(([name, value]) => <div key={name}><dt>{name}</dt><dd><pre>{value}</pre></dd></div>)}</dl>}
  </section>;
}
