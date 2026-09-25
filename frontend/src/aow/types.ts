import type { EditorSettings } from '../features/editor/types';
export interface AowWorktree {
  id: string;
  project_id: string;
  path: string;
  branch: string;
  head: string;
  is_main: boolean;
  detached: boolean;
  locked: boolean;
  prunable: boolean;
  color: WorktreeColor;
  icon?: WorktreeIconId;
}

export type WorktreeColor = 'default' | 'blue' | 'purple' | 'pink' | 'red' | 'orange' | 'yellow' | 'green' | 'teal' | 'gray';

export type WorktreeIconId = 'default'
  | 'cat' | 'dog' | 'rabbit' | 'bird' | 'fish' | 'turtle' | 'squirrel' | 'snail' | 'bug' | 'rat'
  | 'apple' | 'banana' | 'cherry' | 'citrus' | 'grape' | 'pear' | 'peach' | 'strawberry' | 'watermelon' | 'pineapple'
  | 'leaf' | 'sprout' | 'flower' | 'flower_2' | 'clover' | 'wheat' | 'tree_pine' | 'tree_deciduous' | 'tree_palm' | 'trees';

export interface AowProject {
  builtin?: boolean;
  avatar_url?: string | null;
  id: string;
  name: string;
  registered_path: string;
  common_git_dir: string;
  notes_path: string;
  worktrees: AowWorktree[];
  error?: string;
}

export interface AowSettings {
  notes_base: string;
  node_addresses: string[];
  execution_path: string[];
  editor: EditorSettings;
}

export interface WorktreeRemovalPreview {
  worktree: AowWorktree;
  dirty: boolean;
  changes: string[];
  change_count: number;
  truncated: boolean;
  terminal_tabs: number;
  agent_tabs: number;
}

export interface WorktreeRemovalItem {
  path: string;
  force: boolean;
  status: 'queued' | 'running' | 'succeeded' | 'failed' | 'interrupted';
  error: string | null;
}

export interface WorktreeRemovalJob {
  id: string;
  project_id: string;
  created_at: string;
  items: WorktreeRemovalItem[];
}

export interface PinnedWorktrees {
  paths: string[];
  revision: number;
}

export interface PinnedWorktreesUpdate {
  add?: string[];
  remove?: string[];
  order?: string[];
}
