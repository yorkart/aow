// Port of acp_tools::push_stream_message_for_connection: resolve responses by request direction.
import { object, type DebugMessage } from '../types';

export interface LogMessage {
  key: string;
  source: DebugMessage;
  name: string;
  type: 'Request' | 'Response' | 'Notification' | 'Stderr' | 'Raw';
  requestId?: string | number;
  payload?: unknown;
  raw?: string;
  failed: boolean;
}

export function logMessages(entries: DebugMessage[]): LogMessage[] {
  const requests = new Map<string, string>();
  const messages: LogMessage[] = [];
  const occurrences = new Map<string, number>();
  for (const source of entries) {
    const prefix = JSON.stringify([source.connection_id, source.timestamp, source.direction]);
    const occurrence = occurrences.get(prefix) ?? 0;
    occurrences.set(prefix, occurrence + 1);
    const key = `${prefix}:${occurrence}`;
    if (source.direction === 'stderr') {
      messages.push({ key, source, name: 'stderr', type: 'Stderr', raw: source.message, failed: false });
      continue;
    }
    let parsed: unknown;
    try { parsed = JSON.parse(source.message); }
    catch { messages.push({ key, source, name: 'Raw message', type: 'Raw', raw: source.message, failed: false }); continue; }
    const batch = Array.isArray(parsed) ? parsed : [parsed];
    for (const [index, value] of batch.entries()) {
      const wire = object(value);
      const id = typeof wire.id === 'string' || typeof wire.id === 'number' ? wire.id : undefined;
      const requestKey = (direction: string) => JSON.stringify([source.connection_id, direction, id]);
      const message: LogMessage = { key: `${key}:${index}`, source, name: 'Raw message', type: 'Raw', payload: value, failed: false };
      if (typeof wire.method === 'string') {
        message.name = wire.method;
        message.type = id === undefined ? 'Notification' : 'Request';
        message.requestId = id;
        message.payload = wire.params;
        if (id !== undefined) requests.set(requestKey(source.direction), wire.method);
      } else if (id !== undefined) {
        const opposite = requestKey(source.direction === 'incoming' ? 'outgoing' : 'incoming');
        message.name = requests.get(opposite) ?? source.request_method ?? '[response: request outside exported log]';
        requests.delete(opposite);
        message.type = 'Response';
        message.requestId = id;
        message.failed = Object.hasOwn(wire, 'error');
        message.payload = message.failed ? wire.error : wire.result;
      }
      messages.push(message);
    }
  }
  return messages;
}

export function payloadText(message: LogMessage, expanded: boolean): string | undefined {
  if (message.raw !== undefined) return expanded ? message.raw : message.raw.replace(/\r?\n/g, ' ');
  const pretty = JSON.stringify(message.payload, null, 2);
  // JSON string contents contain escaped newlines; only structural whitespace is compacted.
  return expanded ? pretty : pretty?.replace(/\n\s*/g, ' ');
}
