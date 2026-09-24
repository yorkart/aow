import { FolderOpen, PanelsTopLeft, Undo2 } from 'lucide-react';

const actions = {
  open: { label: '打开', icon: FolderOpen },
  floating: { label: '打开 · 浮动工作区', icon: PanelsTopLeft },
  restore: { label: '还原', icon: Undo2 },
};

export type WorkspaceMenuAction = keyof typeof actions;

export function WorkspaceMenuLabel({ action }: { action: WorkspaceMenuAction }) {
  const { label, icon: Icon } = actions[action];
  return <><Icon size={15} aria-hidden="true" />{label}</>;
}
