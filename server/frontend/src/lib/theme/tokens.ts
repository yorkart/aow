import type { Theme } from './types';

/** Semantic roles shared by desktop, mobile, dialogs and conversation renderers. */
export function themeTokens(theme: Theme): Record<string, string> {
  const c = theme.colors;
  const mix = (foreground: string, amount: number, background = c.background) =>
    `color-mix(in srgb, ${foreground} ${amount}%, ${background})`;
  const tokens: Record<string, string> = {
    '--bg': theme.editor.background, '--panel': c.panel, '--panel2': c.header,
    '--line': c.border, '--accent': c.accent, '--hover': c.hover,
    '--text': c.text, '--muted': c.muted,
    '--aow-surface': c.background, '--aow-panel': c.panel,
    '--aow-sidebar-background': c.sidebar, '--aow-sidebar-selection': c.selection,
    '--aow-worktree-current-background': mix(c.text, 3, c.panel),
    '--aow-header': c.header, '--aow-elevated': c.elevated, '--aow-input': c.input,
    '--aow-line': c.border, '--aow-control-line': c.controlBorder,
    '--aow-hover': c.hover, '--aow-list-highlight': c.hover, '--aow-list-selection': c.selection,
    '--aow-accent': c.accent, '--aow-selection': c.selection,
    '--aow-primary': c.primary, '--aow-primary-hover': c.primaryHover,
    '--aow-on-primary': '#ffffff',
    '--aow-badge-background': c.badgeBackground, '--aow-badge-foreground': c.badgeForeground,
    '--aow-notification-background': c.notificationBackground, '--aow-notification-border': c.notificationBorder,
    '--aow-notification-accent': c.notificationAccent,
    '--aow-danger': mix(c.error, 60, '#000000'),
    '--aow-danger-hover': mix(c.error, 55, '#000000'),
    '--aow-text': c.text, '--aow-text-strong': c.text, '--aow-text-hover': c.text,
    '--aow-text-secondary': c.secondary, '--aow-muted': c.muted,
    '--aow-link': c.link, '--aow-link-hover': c.linkHover,
    '--aow-icon': c.secondary, '--aow-icon-hover': c.text,
    '--aow-icon-active': c.accent, '--aow-icon-disabled': c.disabled,
    '--aow-overlay': theme.appearance === 'dark' ? '#00000080' : '#00000040',
    '--aow-shadow': theme.appearance === 'dark' ? '#00000060' : '#00000026',
    '--aow-subtle': mix(c.text, 4), '--aow-subtle-hover': mix(c.text, 8),
    '--aow-line-subtle': mix(c.text, 10),
    '--aow-scrollbar': mix(c.text, 25),
    '--aow-editor-background': theme.editor.background,
    '--aow-conversation-background': c.background,
    '--aow-conversation-panel': c.panel,
    '--aow-conversation-user-background': c.userBackground,
    '--aow-conversation-user-line': c.userBorder,
  };
  for (const role of ['success', 'error', 'warning', 'info', 'purple'] as const) {
    tokens[`--aow-${role}`] = c[role];
    tokens[`--aow-${role}-background`] = role === 'info' ? c.infoBackground : mix(c[role], 12);
    tokens[`--aow-${role}-border`] = role === 'info' ? c.infoBorder : mix(c[role], 38);
  }
  for (const [role, color] of Object.entries(theme.syntax)) tokens[`--aow-syntax-${role}`] = color;
  return tokens;
}
