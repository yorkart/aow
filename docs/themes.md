# Themes

Open **Settings → Appearance** on desktop, or **设置 → Appearance** on mobile.
Selecting a card applies the theme immediately to the workbench, dialogs,
conversations, Monaco editors and diffs, and Mermaid diagrams. Terminal colors
are configured independently under **终端配色** in the same Appearance settings.
It preserves editor models, undo history, terminal buffers, and connections.

The selection is saved in this browser's local storage and synchronized between
tabs of the same origin and deployment path. It is not an account or server
setting. Missing or unknown selections fall back to **AOW Dark**. If storage is
unavailable, switching still works and the settings page explains that it cannot
save the choice.

## Included palettes

| Theme | Reference |
| --- | --- |
| AOW Dark | Existing AOW colors, consolidated into shared semantic roles |
| JetBrains Dark | JetBrains Themes for Zed 1.4.0 |
| JetBrains Light | JetBrains Themes for Zed 1.4.0 |
| JetBrains Islands Dark | JetBrains Themes for Zed 1.4.0 |
| JetBrains Islands Light | JetBrains Themes for Zed 1.4.0 |
| VS Code Dark Modern | VSCode Dark Modern for Zed 0.1.6 |

Reference palettes were read from the installed Zed extensions:

- [JetBrains Themes](https://github.com/artemevsevev/zed-theme-jetbrains),
  Copyright 2025 Artem Evsevev, Apache-2.0.
- [VSCode Dark Modern](https://github.com/kcamcam/vscode_dark_modern.zed),
  Copyright 2024 Kevin Camellini, MIT.

License texts and the upstream JetBrains notice are preserved in
`frontend/public/third-party/themes/` and included in the frontend build.

These are AOW adaptations. They retain the reference editor colors,
while mapping interface roles to AOW's layout and adjusting secondary text,
links, and status colors for readability. Dark themes currently use matching AOW
UI blues for links, active icons, selections, buttons, focus borders and user
messages. Each theme declares its own values, including hover colors and blue
backgrounds/borders; there is no inheritance or reference to another theme.
Editor syntax and terminal palettes remain independent. They do not require Zed
at runtime.

## Theme contract

Theme code lives in `frontend/src/lib/theme/`:

- `types.ts` defines the workbench palette: UI, editor, and syntax colors.
- `terminalPalette.ts` defines independent terminal palettes and their saved preference.
- `themes.ts` registers the built-in palettes with stable IDs.
- `tokens.ts` maps a palette to semantic CSS properties, including derived status
  backgrounds, borders, and overlays.
- `defaults.css` provides the default palette before application initialization
  and for standalone component fixtures. The theme tests check that its values
  match `themeTokens(themes[0])`.
- `store.ts` applies properties on the document root before React renders and
  handles persistence, fallback, and storage events. `useTheme.ts` subscribes
  React consumers without coupling theme data to application components.
- `monacoTheme.ts` adapts theme data for Monaco when the editor is loaded.
  Terminals subscribe to theme and terminal-palette changes and update their
  options in place.

Use roles such as `--aow-surface`, `--aow-text`, `--aow-line`, `--aow-selection`,
`--aow-primary`, `--aow-on-primary`, and `--aow-error`. Prefer an existing role over
adding a new color. Component-specific aliases may reference these roles but
must not override them with fixed dark colors. Fixed artwork, user-assigned
label/worktree colors, QR backgrounds, and alpha masks are independent of themes.

To add a palette, add a complete `Theme` to `themes.ts`; the settings cards and
adapters use this registry automatically. Keep IDs stable so existing preferences
continue to resolve. When adding semantic roles or changing AOW Dark, update
`defaults.css` to match `themeTokens(themes[0])`.

## Validation and preview

```sh
npm --prefix frontend run test:themes
npm --prefix frontend run preview:themes
```

The preview serves the real desktop/mobile app shell with isolated API fixtures
at port 5198. `/tests/theme-preview.html` places the actual theme selector,
editable Monaco instance, diff editor, terminal, and conversation renderer together
for visual verification. It does not access user repositories or start an agent.

The regression suite checks all six palettes, live editor and terminal state,
ANSI colors, Mermaid rendering, desktop/mobile selection, keyboard navigation,
reload persistence, cross-tab updates, deployment isolation, unavailable storage,
invalid preferences, and the default CSS contract.

## Terminal palettes

Choose **跟随主题**, **柔和深色**, or **柔和浅色** under **终端配色**.
The default follows the workbench's light/dark appearance and uses the matching
soft palette. An explicit selection stays fixed when the workbench theme changes.
Soft Dark restores the pre-theme terminal's Tango ANSI colors, AOW background,
and selection; Soft Light uses muted colors designed for a light background.
Each palette defines the background, foreground, cursor, selection and all 16
ANSI colors. Applications using explicit RGB or 256-color output keep those colors.

The `aow.terminal-palette` preference is saved separately in browser storage, scoped
to the deployment path, and synchronized across tabs. Missing or invalid values
fall back to following the theme. Blocked storage still allows immediate changes
and displays a warning. Updates preserve live terminal connections and buffers.

## Diff palettes

**Diff 配色** offers **跟随主题**, **深色**, and **浅色** independently of the
workbench and terminal palettes. Auto follows the workbench's light/dark mode;
manual choices remain fixed. Dark restores Monaco's original `vs-dark` red/green
line and text backgrounds. Each palette also sets gutter and overview markers. Mobile text diffs use the
same independent line backgrounds while retaining workbench text colors.
Code backgrounds and syntax colors continue to follow the editor theme.

The `aow.diff-palette` preference uses deployment-scoped browser storage and
synchronizes across tabs. Missing/invalid values follow the theme; blocked storage
allows immediate changes and displays a warning. Existing Git, PR and ACP Monaco
diffs update in place without recreating models or losing edits and undo history.

## Notification contrast

Each workbench theme owns `badgeBackground`, `badgeForeground`,
`notificationBackground`, `notificationBorder`, and `notificationAccent`.
Unread worktree counts use solid blue pills with white text. Notification cards
use a raised neutral surface, a visible border, a soft shadow, and an accent stripe.
Dark themes currently declare matching values independently; light themes use
white notification surfaces and darker borders.
