# Themes

Open **Settings → Appearance** on desktop, or **设置 → Appearance** on mobile.
Selecting a card applies the theme immediately to the workbench, dialogs,
conversations, Monaco editors and diffs, Mermaid diagrams, and terminal ANSI colors.
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

These are AOW adaptations. They retain the reference editor and terminal colors,
while mapping interface roles to AOW's layout and adjusting secondary text,
links, and status colors for readability. They do not require Zed at runtime.

## Theme contract

Theme code lives in `frontend/src/lib/theme/`:

- `types.ts` defines a complete palette: UI, editor, terminal, and syntax colors.
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
  Terminals subscribe to the same store and update their options in place.

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
