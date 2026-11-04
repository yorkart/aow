// Monaco's matching, bundled language pack must execute before editor modules
// register their actions. A page reload applies language changes consistently.
// Monaco 0.56 does not export its locale directory through package exports.
import '../../../node_modules/monaco-editor/dev/vs/nls/lang/zh-cn.js';
import { getLanguage } from '../../lib/i18n';

const nls = globalThis as typeof globalThis & {
  _VSCODE_NLS_LANGUAGE?: string;
  _VSCODE_NLS_MESSAGES?: string[];
};
if (getLanguage() !== 'zh-CN') {
  delete nls._VSCODE_NLS_LANGUAGE;
  delete nls._VSCODE_NLS_MESSAGES;
}
