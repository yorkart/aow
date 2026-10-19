import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';

export { monaco };
import { conf as javascriptConfiguration, language as javascriptLanguage } from 'monaco-editor/languages/definitions/javascript/javascript.js';
import { conf as pythonConfiguration, language as pythonLanguage } from 'monaco-editor/languages/definitions/python/python.js';
import { conf as typescriptConfiguration, language as typescriptLanguage } from 'monaco-editor/languages/definitions/typescript/typescript.js';
import editorWorker from 'monaco-editor/editor/editor.worker.js?worker';
import cssWorker from 'monaco-editor/languages/features/css/css.worker.js?worker';
import htmlWorker from 'monaco-editor/languages/features/html/html.worker.js?worker';
import jsonWorker from 'monaco-editor/languages/features/json/json.worker.js?worker';
import tsWorker from 'monaco-editor/languages/features/typescript/ts.worker.js?worker';
import { tomlConfiguration, tomlLanguage } from './toml';

// Both the editor workspace and PR inline diffs use Monaco. Keep its worker
// configuration in one module imported by each lazy editor entry, so a DiffEditor
// also works when no normal file editor has been opened yet.
self.MonacoEnvironment = {
  getWorker(_moduleId: string, label: string) {
    if (label === 'json') return new jsonWorker();
    if (label === 'css' || label === 'scss' || label === 'less') return new cssWorker();
    if (label === 'html' || label === 'handlebars' || label === 'razor') return new htmlWorker();
    if (label === 'typescript' || label === 'javascript') return new tsWorker();
    return new editorWorker();
  },
};

monaco.languages.setMonarchTokensProvider('typescript', typescriptLanguage);
monaco.languages.setLanguageConfiguration('typescript', typescriptConfiguration);
monaco.languages.setMonarchTokensProvider('javascript', javascriptLanguage);
monaco.languages.setLanguageConfiguration('javascript', javascriptConfiguration);
monaco.languages.setMonarchTokensProvider('python', pythonLanguage);
monaco.languages.setLanguageConfiguration('python', pythonConfiguration);
monaco.languages.register({ id: 'toml', extensions: ['.toml'], aliases: ['TOML', 'toml'] });
monaco.languages.setMonarchTokensProvider('toml', tomlLanguage);
monaco.languages.setLanguageConfiguration('toml', tomlConfiguration);
// Reuse JSON's tokenizer, formatting and completion for JSONC, with syntax
// allowances scoped to its file extension so ordinary JSON stays strict.
const jsonDiagnostics = monaco.json.jsonDefaults.diagnosticsOptions;
monaco.json.jsonDefaults.setDiagnosticsOptions({
  ...jsonDiagnostics,
  schemas: [...(jsonDiagnostics.schemas ?? []), {
    uri: 'aow://schemas/jsonc',
    fileMatch: ['**/*.[jJ][sS][oO][nN][cC]'],
    schema: { allowComments: true, allowTrailingCommas: true },
  }],
});
loader.config({ monaco });
