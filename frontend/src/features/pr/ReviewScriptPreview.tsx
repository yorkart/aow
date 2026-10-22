import { t } from '../../lib/i18n';
import Editor from '../editor/MonacoEditor';

export default function ReviewScriptPreview({ value }: { value: string }) {
  return <div className="review-provider-editor"><Editor height="360px" language="python" value={value}
    options={{ readOnly: true, domReadOnly: true, readOnlyMessage: { value: t("pr.scripts_are_preview_only_upload_a_replacement_file") }, automaticLayout: true, minimap: { enabled: false }, fontSize: 13, scrollBeyondLastLine: false, tabFocusMode: true, ariaLabel: t("pr.provider_python_script_preview"), wordWrap: 'off' }} /></div>;
}
