# VS Code Seti file icons

`seti.woff` and `vs-seti-icon-theme.json` are vendored without modification from
[VS Code 1.134.0's built-in Seti theme](https://github.com/microsoft/vscode/tree/1.134.0/extensions/theme-seti).
The theme records its [Seti UI source revision](https://github.com/jesseweed/seti-ui/commit/2d6c5e68b4ded73c92dac291845ee44e1182d511).
The original Seti MIT notice and the VS Code MIT license are in
[`public/third-party/seti`](../../../public/third-party/seti), so Vite also includes
them in every production build.

`language-associations.json` supplies the language detection that VS Code normally
provides to the icon theme. It is derived from the same version's built-in
`extensions/*/package.json` manifests: for each `contributes.languages` entry
whose `id` occurs in the theme's `languageIds`, map its `extensions` (without the
leading dot), `filenames`, and basename-only `filenamePatterns` to that icon ID.
Keys are lowercase and sorted. Path-only language hints are omitted because they
use the same JSON/Markdown icons as their file extensions.

When updating, copy the font and theme together and regenerate those associations
from the matching VS Code version. Explicit Seti filenames and compound suffixes
take precedence over the language associations. The UI uses Seti's original dark
colors and 150% font size inside a 16px slot, with neutral outline folders because
Seti does not define folder icons. Vite bundles the font locally for offline and
subpath deployments.
