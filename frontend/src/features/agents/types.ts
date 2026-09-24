export type AowAgentType = 'claude' | 'codex' | 'traecli';

export interface AowAgent {
  id: string;
  agent_type: AowAgentType | null;
  display_name: string;
  source: 'detected' | 'configured';
  available: boolean;
  command: string;
  executable: string | null;
  args: string[];
  env: Record<string, string>;
}
