import { t } from '../../lib/i18n';
import { acpApi } from './api';
import { savedAcpTabs } from './agent_ui/workspace_tabs';

// Resolve persisted session identity without connecting an adapter or loading its transcript.
export async function resolveAcpSession(thread_id: string, signal?: AbortSignal) {
  const session = await acpApi.info(thread_id, signal);
  if (!session || session.thread_id !== thread_id) throw new Error(t("acp.this_acp_session_no_longer_exists"));
  // A newly created view keeps its draft ID across portal moves; the URL uses the session ID.
  const tabId = savedAcpTabs(session.cwd).find(tab => tab.thread_id === thread_id)?.id ?? thread_id;
  return { session, tabId };
}
