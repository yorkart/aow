import { useEffect, useState } from 'react';
import { CircleDot, ExternalLink, Filter, RefreshCw, Settings2 } from 'lucide-react';
import { AowPanel } from '../../components/AowPanel';
import { AowIconButton } from '../../components/AowIconButton';
import { SourceSettingsDialog } from './SourceSettingsDialog';
import { sourcePath, useSourceChanges, useSourceQuery, type IssueLabel, type IssueList, type SourceSettings } from './sources';
import './tasks.css';

export function IssuePanel({ projectId, visible }: { projectId: string; visible: boolean }) {
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [status, setStatus] = useState('open');
  const [refresh, setRefresh] = useState(0);
  const [limit, setLimit] = useState(50);
  const changes = useSourceChanges();
  const path = sourcePath(projectId);
  const settings = useSourceQuery<SourceSettings>(path, visible, `${changes}:${refresh}`, undefined, true);
  const source = settings.data?.sources.find(source => source.type === 'repository_issues');
  const enabled = visible && source?.type === 'repository_issues' && source.enabled;
  const identity = JSON.stringify([source, settings.data?.revision, refresh]);
  const labels = useSourceQuery<{ labels: IssueLabel[] }>(`${path}/labels`, enabled, identity);
  const issues = useSourceQuery<IssueList>(`${path}/issues`, enabled, identity, JSON.stringify({ state: status, labels: selected }));
  useEffect(() => { setLimit(50); }, [selected, status, identity]);
  const error = settings.error || issues.error;
  const busy = settings.loading || issues.loading || labels.loading;
  const reload = () => setRefresh(value => value + 1);
  useEffect(() => {
    const update = () => setRefresh(value => value + 1);
    window.addEventListener('aow-review-providers-changed', update);
    return () => window.removeEventListener('aow-review-providers-changed', update);
  }, []);
  const names = [...new Set([...(labels.data?.labels.map(label => label.name) ?? []), ...selected])];
  return <>
    <AowPanel title="Issue" icon={<CircleDot />} details={issues.data && <span>{issues.data.issues.length}</span>} actions={<>
      <AowIconButton aria-label="按 Label 筛选 Issue" title="按 Label 筛选 Issue" aria-expanded={filtersOpen} onClick={() => setFiltersOpen(value => !value)} disabled={!enabled}><Filter /></AowIconButton>
      <AowIconButton aria-label="刷新 Issue" title="刷新 Issue" disabled={busy} onClick={reload}><RefreshCw /></AowIconButton>
      <AowIconButton aria-label="需求源配置" title="需求源配置" disabled={!settings.data} onClick={() => setSettingsOpen(true)}><Settings2 /></AowIconButton>
    </>}>
      {enabled && <div className="tasks-issue-toolbar"><span title={issues.data ? `${issues.data.provider_name} · ${issues.data.remote}` : undefined}>{issues.data ? `${issues.data.provider_name} · ${issues.data.remote}` : '仓库共享需求'}</span>
        <select aria-label="Issue 状态" value={status} onChange={event => setStatus(event.target.value)}><option value="open">Open</option><option value="closed">Closed</option><option value="all">全部状态</option></select>
      </div>}
      {enabled && (filtersOpen || selected.length > 0) && <div className="tasks-issue-filters">
        <div><span>{selected.length ? `已选 ${selected.length} 个 Label · 匹配任意一个` : 'Label · 可多选，匹配任意一个'}</span>{selected.length > 0 && <button type="button" onClick={() => setSelected([])}>清除筛选</button>}</div>
        {filtersOpen && <>
          {labels.loading && <p className="tasks-hint">正在加载 Label…</p>}
          {labels.error && <p role="alert" className="tasks-error">{labels.error}<button type="button" onClick={reload}>重试</button></p>}
          {!labels.loading && !labels.error && !names.length && <p className="tasks-hint">仓库暂无 Label</p>}
          <div className="tasks-label-options" role="group" aria-label="Issue Labels">{names.map(name => {
            const label = labels.data?.labels.find(label => label.name === name);
            return <label key={name} title={label?.description || name}><input type="checkbox" checked={selected.includes(name)} onChange={event => setSelected(current => event.target.checked ? [...current, name] : current.filter(item => item !== name))} /><span className="tasks-label-dot" style={{ background: label?.color ? `#${label.color}` : undefined }} /><span>{name}</span></label>;
          })}</div>
        </>}
      </div>}
      {error && <p role="alert" className="tasks-error">{error}<button type="button" className="tasks-source-retry" onClick={reload}>重试</button></p>}
      {(!settings.data && settings.loading) || issues.loading ? <p className="tasks-empty" role="status">正在加载 Issue…</p> : source?.type === 'repository_issues' && !source.enabled ? <div className="tasks-empty"><p>Issue 需求源已停用</p><button onClick={() => setSettingsOpen(true)}>配置需求源</button></div> : issues.data && <>
        {!issues.data.issues.length ? <p className="tasks-empty">{selected.length ? '没有匹配所选 Label 的 Issue' : '暂无符合条件的 Issue'}</p> : <ul className="tasks-issue-list">{issues.data.issues.slice(0, limit).map(issue => <li key={issue.number}>
          <div className="tasks-issue-title"><CircleDot size={14} className={`tasks-issue-${issue.status}`} />{issue.url ? <a href={issue.url} target="_blank" rel="noopener noreferrer" title={issue.title}>{issue.title}<ExternalLink size={12} /></a> : <span>{issue.title}</span>}</div>
          <div className="tasks-issue-meta"><span>#{issue.number}</span><span>{issue.status === 'open' ? 'Open' : 'Closed'}</span>{issue.assignees.length > 0 && <span title={issue.assignees.join(', ')}>@{issue.assignees.join(', @')}</span>}<time dateTime={issue.updated_at} title={new Date(issue.updated_at).toLocaleString('zh-CN')}>{new Date(issue.updated_at).toLocaleDateString('zh-CN')}</time></div>
          {issue.labels.length > 0 && <div className="tasks-issue-labels">{issue.labels.map(label => <span key={label}>{label}</span>)}</div>}
        </li>)}</ul>}
        {issues.data.issues.length > limit && <div className="tasks-empty"><button onClick={() => setLimit(value => value + 50)}>显示更多 Issue</button></div>}
      </>}
    </AowPanel>
    {settingsOpen && settings.data && <SourceSettingsDialog projectId={projectId} settings={settings.data} onClose={() => setSettingsOpen(false)} onSaved={() => { setSettingsOpen(false); setSelected([]); reload(); }} />}
  </>;
}
