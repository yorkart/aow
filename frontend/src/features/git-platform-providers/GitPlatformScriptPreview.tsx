import { t } from '../../lib/i18n';
import Editor from '../editor/MonacoEditor';

export default function GitPlatformScriptPreview({ value }: { value: string }) {
  return <div className="git-platform-provider-editor"><Editor height="360px" language="javascript" value={value}
    options={{ readOnly: true, domReadOnly: true, readOnlyMessage: { value: t("git_platform.scripts_are_preview_only_upload_a_replacement_file") }, automaticLayout: true, minimap: { enabled: false }, fontSize: 13, scrollBeyondLastLine: false, tabFocusMode: true, ariaLabel: t("git_platform.provider_mjs_script_preview"), wordWrap: 'off' }} /></div>;
}
