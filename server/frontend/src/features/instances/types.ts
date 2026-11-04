export interface InstanceRef { kind: 'terminal' | 'acp'; id: string }
export interface InstanceInfo extends InstanceRef {
  parent: InstanceRef | null; title: string; cwd: string; agent_id: string | null;
  url: string | null; deleted: boolean;
  tab_id?: string | null;
}
export const instanceKey = (instance: InstanceRef) => `${instance.kind}:${instance.id}`;
