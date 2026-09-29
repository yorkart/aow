import { aowRequest } from './aowRequest';

// Allocate through the server so tabs, devices and CLI callers share one
// Snowflake namespace. Keep the string intact; it exceeds JS number precision.
export async function allocateId(): Promise<string> {
  const { id } = await aowRequest<{ id: string }>('/api/ids', { method: 'POST' });
  return id;
}
