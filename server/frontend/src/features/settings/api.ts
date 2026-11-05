import type { AowSettings, PinnedWorktrees, PinnedWorktreesUpdate } from '../../aow/types';
import { aowRequest } from '../../lib/aowRequest';

export const settingsApi = {
  pinnedWorktrees: () => aowRequest<PinnedWorktrees>('/api/pinned-worktrees', { cache: 'no-store' }),
  updatePinnedWorktrees: (input: PinnedWorktreesUpdate) => aowRequest<PinnedWorktrees>('/api/pinned-worktrees', {
    method: 'PATCH', body: JSON.stringify(input),
  }),
  settings: () => aowRequest<AowSettings>('/api/settings', { cache: 'no-store' }),
  discoveredPath: () => aowRequest<string[]>('/api/settings/discovered-path', { cache: 'no-store' }),
  updateSettings: (input: { notesBase?: string; nodeAddresses?: string[]; executionPath?: string[]; editor?: AowSettings['editor'] }) => aowRequest<AowSettings>('/api/settings', {
    method: 'PUT',
    body: JSON.stringify({ notes_base: input.notesBase?.trim(), node_addresses: input.nodeAddresses, execution_path: input.executionPath, editor: input.editor }),
  }),
};
