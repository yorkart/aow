import { useCallback, useId, useState, type ReactNode } from 'react';
import { ChevronRight, MoreHorizontal, RefreshCw } from 'lucide-react';
import type { AowWorktree } from '../../aow/types';
import { AowIconButton } from '../../components/AowIconButton';
import { AowPanel, AowPanelStack } from '../../components/AowPanel';
import { InstanceScopeMenu, type InstanceSort } from './InstanceScopeMenu';
import './instance-panel.css';

export type InstanceGroup = 'user' | 'cli';
export type InstanceScopes = Record<InstanceGroup, boolean>;
export type InstanceSorts = Record<InstanceGroup, InstanceSort>;
export interface InstancePanelGroup<T> {
  id: InstanceGroup; title: string; icon: ReactNode; tooltip: ReactNode; items: T[];
  actions?: ReactNode;
  loading?: boolean;
  error?: string;
}
function worktreeName(worktree: AowWorktree) {
  return worktree.detached ? 'detached'
    : worktree.branch || worktree.path.split('/').filter(Boolean).at(-1) || worktree.path;
}

export function InstancePanelGroups<T>({ groups, worktrees, activeWorktreePath, showAll, sortBy,
  onShowAllChange, onSortChange, onReload, onScopeMenuOpen, loading = false, error, noun, empty,
  itemKey, itemWorkspace, itemCreatedAt, renderItem, menuContainer }: {
  groups: InstancePanelGroup<T>[];
  worktrees?: AowWorktree[];
  activeWorktreePath?: string;
  showAll: InstanceScopes;
  sortBy: InstanceSorts;
  onShowAllChange?: (group: InstanceGroup, value: boolean) => void;
  onSortChange?: (group: InstanceGroup, value: InstanceSort) => void;
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
  const [collapsedGroups, setCollapsedGroups] = useState<Partial<Record<InstanceGroup, boolean>>>({});
  const [collapsedWorktrees, setCollapsedWorktrees] = useState<Record<string, boolean>>({});
  const [scopeMenu, setScopeMenu] = useState<{ anchor: HTMLButtonElement; group: InstanceGroup }>();
  const dismissScopeMenu = useCallback(() => setScopeMenu(undefined), []);
  return <>
    <AowPanelStack className="instance-panel-groups">
      {groups.map(({ id, title, icon, tooltip, items, actions, loading: groupLoading = loading, error: groupError = error }) => {
        const grouped = showAll[id] && !!worktrees;
        const collapsed = collapsedGroups[id] ?? (items.length === 0 && !groupLoading && !groupError);
        const bodyId = `${panelId}-${id}`;
        const branches = grouped ? [...worktrees]
          .sort((left, right) => Number(right.is_main) - Number(left.is_main)
            || worktreeName(left).localeCompare(worktreeName(right)))
          .map(worktree => ({ key: `worktree:${worktree.path}`, worktree, members: items.filter(item => itemWorkspace(item) === worktree.path) }))
          .filter(group => group.members.length) : [];
        const sections = sortBy[id] === 'createdAt'
          ? branches.flatMap(group => group.members.map(item => ({ ...group, key: `instance:${itemKey(item)}`, members: [item] })))
            .sort((left, right) => itemCreatedAt(left.members[0]) - itemCreatedAt(right.members[0]))
          : branches;
        return <AowPanel key={id} className="instance-panel-group" bodyClassName="instance-panel-body" title={title} tooltip={tooltip} icon={icon} collapsed={collapsed} controlsId={bodyId}
          onCollapsedChange={next => setCollapsedGroups(groups => ({ ...groups, [id]: next }))}
          details={grouped ? <span className="instance-panel-count">{items.length}</span> : undefined}
          actions={<>
            <AowIconButton title={`刷新${noun}列表`} aria-label={`刷新 ${title}`} onClick={onReload}><RefreshCw /></AowIconButton>
            {actions}
            {worktrees && onShowAllChange ? <AowIconButton title={`${noun}显示范围`} aria-label={`${title} 显示范围`}
              aria-haspopup="menu" aria-expanded={scopeMenu?.group === id}
              className={grouped ? 'instance-scope-active' : undefined}
              onClick={event => {
                onScopeMenuOpen?.();
                const anchor = event.currentTarget;
                setScopeMenu(current => current?.group === id ? undefined : { anchor, group: id });
              }}><MoreHorizontal /></AowIconButton> : null}
          </>}>
          {groupError ? <p className="instance-panel-error" role="alert">{groupError}</p> : null}
          {groupLoading ? <p className="instance-panel-empty" role="status">正在加载{noun}…</p> : null}
          {!items.length && !groupLoading ? <p className="instance-panel-empty">{empty ?? `暂无${noun}`}</p> : grouped ? sections
            .map(({ key: sectionKey, worktree, members }, index) => {
              const key = `${id}:${sectionKey}`;
              const collapsed = collapsedWorktrees[key] ?? false;
              const groupId = `${bodyId}-worktree-${index}`;
              const active = worktree.path === activeWorktreePath;
              return <section className={`instance-worktree-group${active ? ' active' : ''}`} key={sectionKey} aria-label={worktree.path}>
                <button className="instance-worktree-heading" title={`${worktree.path}${worktree.branch ? `\n${worktree.branch}` : ''}`}
                  aria-current={active ? 'true' : undefined} aria-expanded={!collapsed} aria-controls={groupId}
                  onClick={() => setCollapsedWorktrees(groups => ({ ...groups, [key]: !groups[key] }))}>
                  <ChevronRight className={collapsed ? undefined : 'expanded'} />
                  <span>{worktree.is_main ? '主仓库 · ' : ''}{worktreeName(worktree)}</span>
                  <small className="instance-panel-count">{members.length}</small>
                </button>
                <div id={groupId} hidden={collapsed}>{members.map(renderItem)}</div>
              </section>;
            }) : items.map(renderItem)}
        </AowPanel>;
      })}
    </AowPanelStack>
    {scopeMenu && worktrees && onShowAllChange ? <InstanceScopeMenu label={`${noun}显示范围`} container={menuContainer} anchor={scopeMenu.anchor} checked={showAll[scopeMenu.group]}
      sortBy={sortBy[scopeMenu.group]} onSortChange={value => onSortChange?.(scopeMenu.group, value)}
      onChange={value => onShowAllChange(scopeMenu.group, value)} onClose={dismissScopeMenu} /> : null}
  </>;
}
