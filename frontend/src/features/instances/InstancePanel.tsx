import { t } from '../../lib/i18n';
import { useCallback, useId, useState, type ReactNode } from 'react';
import { ChevronRight, MoreHorizontal, RefreshCw } from 'lucide-react';
import type { AowWorktree } from '../../aow/types';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { InstanceScopeMenu, type InstanceSort } from './InstanceScopeMenu';
import './instance-panel.css';

function worktreeName(worktree: AowWorktree) {
  return worktree.detached ? 'detached'
    : worktree.branch || worktree.path.split('/').filter(Boolean).at(-1) || worktree.path;
}

export function InstancePanel<T>({ title, icon, items, actions, worktrees, activeWorktreePath, showAll, sortBy,
  onShowAllChange, onSortChange, onReload, onScopeMenuOpen, loading = false, error, noun, empty,
  itemKey, itemWorkspace, itemCreatedAt, renderItem, menuContainer }: {
  title: string;
  icon: ReactNode;
  items: T[];
  actions?: ReactNode;
  worktrees?: AowWorktree[];
  activeWorktreePath?: string;
  showAll: boolean;
  sortBy: InstanceSort;
  onShowAllChange?: (value: boolean) => void;
  onSortChange?: (value: InstanceSort) => void;
  onReload: () => void;
  onScopeMenuOpen?: () => void;
  loading?: boolean;
  error?: string;
  noun: string;
  empty?: string;
  itemKey: (item: T) => string;
  itemWorkspace: (item: T) => string;
  itemCreatedAt: (item: T) => number;
  renderItem: (item: T) => ReactNode;
  menuContainer?: HTMLElement;
}) {
  const panelId = useId();
  const [collapsedWorktrees, setCollapsedWorktrees] = useState<Record<string, boolean>>({});
  const [scopeMenu, setScopeMenu] = useState<HTMLButtonElement>();
  const dismissScopeMenu = useCallback(() => setScopeMenu(undefined), []);
  const grouped = showAll && !!worktrees;
  const branches = grouped ? [...worktrees]
    .sort((left, right) => Number(right.is_main) - Number(left.is_main)
      || worktreeName(left).localeCompare(worktreeName(right)))
    .map(worktree => ({ key: `worktree:${worktree.path}`, worktree, members: items.filter(item => itemWorkspace(item) === worktree.path) }))
    .filter(group => group.members.length) : [];
  const sections = sortBy === 'createdAt'
    ? branches.flatMap(group => group.members.map(item => ({ ...group, key: `instance:${itemKey(item)}`, members: [item] })))
      .sort((left, right) => itemCreatedAt(left.members[0]) - itemCreatedAt(right.members[0]))
    : branches;
  return <>
    <AowPanelStack className="instance-panel-groups">
      <AowPanel className="instance-panel-group" bodyClassName="instance-panel-body" title={title} icon={icon}
        empty={items.length === 0 && !loading && !error} controlsId={panelId}
        details={grouped ? <span className="instance-panel-count">{items.length}</span> : undefined}
        actions={<>
          <AowIconButton title={t("instances.refresh_list", { value0: noun })} aria-label={t("files.refresh_2", { value0: title })} onClick={onReload}><RefreshCw /></AowIconButton>
          {actions}
          {worktrees && onShowAllChange ? <AowIconButton title={t("instances.scope", { value0: noun })} aria-label={t("instances.display_scope", { value0: title })}
            aria-haspopup="menu" aria-expanded={!!scopeMenu}
            className={grouped ? 'instance-scope-active' : undefined}
            onClick={event => {
              onScopeMenuOpen?.();
              const anchor = event.currentTarget;
              setScopeMenu(current => current ? undefined : anchor);
            }}><MoreHorizontal /></AowIconButton> : null}
        </>}>
        {error ? <p className="instance-panel-error" role="alert">{error}</p> : null}
        {loading ? <p className="instance-panel-empty" role="status">{t("instances.loading")}{noun}…</p> : null}
        {!items.length && !loading ? <p className="instance-panel-empty">{empty ?? t("instances.no", { value0: noun })}</p> : grouped ? sections
          .map(({ key: sectionKey, worktree, members }, index) => {
            const collapsed = collapsedWorktrees[sectionKey] ?? false;
            const groupId = `${panelId}-worktree-${index}`;
            const active = worktree.path === activeWorktreePath;
            return <section className={`instance-worktree-group${active ? ' active' : ''}`} key={sectionKey} aria-label={worktree.path}>
              <button className="instance-worktree-heading" title={`${worktree.path}${worktree.branch ? `\n${worktree.branch}` : ''}`}
                aria-current={active ? 'true' : undefined} aria-expanded={!collapsed} aria-controls={groupId}
                onClick={() => setCollapsedWorktrees(groups => ({ ...groups, [sectionKey]: !groups[sectionKey] }))}>
                <ChevronRight className={collapsed ? undefined : 'expanded'} />
                <span>{worktree.is_main ? t("instances.main_repository") : ''}{worktreeName(worktree)}</span>
                <small className="instance-panel-count">{members.length}</small>
              </button>
              <div id={groupId} hidden={collapsed}>{members.map(renderItem)}</div>
            </section>;
          }) : items.map(renderItem)}
      </AowPanel>
    </AowPanelStack>
    {scopeMenu && worktrees && onShowAllChange ? <InstanceScopeMenu label={t("instances.scope", { value0: noun })} container={menuContainer} anchor={scopeMenu} checked={showAll}
      sortBy={sortBy} onSortChange={value => onSortChange?.(value)}
      onChange={onShowAllChange} onClose={dismissScopeMenu} /> : null}
  </>;
}
