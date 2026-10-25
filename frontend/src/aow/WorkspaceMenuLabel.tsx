import { t } from '../lib/i18n';
import { FolderOpen, PanelsTopLeft, Undo2 } from 'lucide-react';

const actions = {
  open: { label: t("aow.open"), icon: FolderOpen },
  floating: { label: t("aow.open_floating_workspace"), icon: PanelsTopLeft },
  restore: { label: t("aow.restore"), icon: Undo2 },
};

export type WorkspaceMenuAction = keyof typeof actions;

export function WorkspaceMenuLabel({ action }: { action: WorkspaceMenuAction }) {
  const { label, icon: Icon } = actions[action];
  return <><Icon size={15} aria-hidden="true" />{label}</>;
}
