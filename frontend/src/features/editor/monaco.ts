import { loader } from '@monaco-editor/react';
import * as monaco from 'monaco-editor';

export { monaco };
import { conf as javascriptConfiguration, language as javascriptLanguage } from 'monaco-editor/esm/vs/basic-languages/javascript/javascript.js';
import { conf as pythonConfiguration, language as pythonLanguage } from 'monaco-editor/esm/vs/basic-languages/python/python.js';
import { conf as typescriptConfiguration, language as typescriptLanguage } from 'monaco-editor/esm/vs/basic-languages/typescript/typescript.js';
import editorWorker from 'monaco-editor/esm/vs/editor/editor.worker?worker';
import cssWorker from 'monaco-editor/esm/vs/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/esm/vs/language/html/html.worker?worker';
import jsonWorker from 'monaco-editor/esm/vs/language/json/json.worker?worker';
import tsWorker from 'monaco-editor/esm/vs/language/typescript/ts.worker?worker';

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
loader.config({ monaco });
