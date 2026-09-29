import { useState } from 'react';
import { TaskDialog } from './TaskDialog';
import { taskError } from './api';
import { saveSources, sourcePath, useSourceQuery, type RequirementSource, type SourceSettings, type SourceTargets } from './sources';

export function SourceSettingsDialog({ projectId, settings, onClose, onSaved }: {
  projectId: string; settings: SourceSettings; onClose: () => void; onSaved: () => void;
}) {
  const [draft, setDraft] = useState(settings);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [refresh, setRefresh] = useState(0);
  const targets = useSourceQuery<SourceTargets>(`${sourcePath(projectId)}/targets`, true, refresh);
  const issue = draft.sources.find((source): source is Extract<RequirementSource, { type: 'repository_issues' }> => source.type === 'repository_issues')!;
  const update = (values: Partial<typeof issue>) => setDraft({ ...draft, sources: draft.sources.map(source => source.type === 'repository_issues' ? { ...source, ...values } : source) });
  const selected = issue.remote ? JSON.stringify([issue.provider, issue.remote]) : '';
  const options = targets.data?.targets ?? [];
  return <TaskDialog title="需求源配置" busy={busy} onClose={onClose}>
    <form onSubmit={event => {
      event.preventDefault(); setBusy(true); setError('');
      void saveSources(projectId, draft).then(onSaved).catch(error => setError(taskError(error))).finally(() => setBusy(false));
    }}>
      <fieldset disabled={busy}>
        <div className="tasks-source-setting"><strong>Inbox</strong><p className="tasks-hint">内置本地需求录入，始终启用。</p></div>
        <div className="tasks-source-setting"><strong>仓库 Issue</strong>
          <label className="tasks-check"><input type="checkbox" checked={issue.enabled} onChange={event => update({ enabled: event.target.checked })} />启用 Issue 需求源</label>
          {targets.data && <p className="tasks-source-repository" title={targets.data.repository}>{targets.data.repository}</p>}
          <label>关联仓库<select aria-label="Issue 关联仓库" value={selected} onChange={event => {
            const target = options.find(target => JSON.stringify([target.provider, target.remote]) === event.target.value);
            update({ provider: target?.provider ?? null, remote: target?.remote ?? null });
          }}>
            <option value="">自动匹配当前项目仓库</option>
            {selected && !options.some(target => JSON.stringify([target.provider, target.remote]) === selected) && <option value={selected}>{issue.provider} · {issue.remote}（不可用）</option>}
            {options.map(target => <option key={JSON.stringify([target.provider, target.remote])} value={JSON.stringify([target.provider, target.remote])}>{target.provider_name} · {target.remote} · {target.host}/{target.repository}</option>)}
          </select></label>
          {targets.loading ? <p className="tasks-hint">正在查找仓库来源…</p> : targets.error ? <p role="alert" className="tasks-error">{targets.error}</p> : !options.length ? <p className="tasks-hint">未找到匹配的仓库来源，请先在 Settings → Pull Requests 配置仓库 Provider。</p> : <p className="tasks-hint">复用仓库 Provider 的连接与登录状态。多个仓库匹配时，请明确选择一个。</p>}
          <button type="button" onClick={() => setRefresh(value => value + 1)} disabled={targets.loading}>刷新仓库来源</button>
        </div>
      </fieldset>
      {error && <p role="alert" className="tasks-error">{error}</p>}
      <footer><button type="button" disabled={busy} onClick={onClose}>取消</button><button type="submit" disabled={busy} className="tasks-primary">{busy ? '正在保存…' : '保存'}</button></footer>
    </form>
  </TaskDialog>;
}
