import type { SessionInfo } from './types';

// Zed sidebar::thread_display_time: background updates do not move a thread.
export const sessionDisplayTime = (session: SessionInfo) => session.interacted_at ?? session.updated_at;
export const compareSessionActivity = (a: SessionInfo, b: SessionInfo) =>
  (Date.parse(sessionDisplayTime(b)) || 0) - (Date.parse(sessionDisplayTime(a)) || 0);
