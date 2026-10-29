import { readFile, stat } from 'node:fs/promises';
import { join } from 'node:path';
import type { Connectors } from '../connectors/service.js';
import { sourceRoot } from '../connectors/identity.js';
import { HttpError } from '../errors.js';
import { defaultRuntime, type AgentRuntime, type ModelOption } from './types.js';

export async function runtimeOptions(connectors: Connectors, org: string, id: string) {
  const connector = connectors.get(org, id);
  const models = new Map<string, ModelOption>();
  const compatible = connector.executor === 'codex' ? /^(?:gpt-|o[134](?:-|$))/ : connector.provider === 'deepseek' ? /^deepseek-/ : connector.executor === 'claude' ? /^claude-/ : /^gemini-/;
  for (const model of connector.models) if (model.length < 160 && compatible.test(model)) models.set(model, { id: model, reasoning_efforts: [] });
  if (connector.executor === 'claude' && connector.provider === 'anthropic') for (const id of ['sonnet', 'opus', 'haiku']) models.set(id, { id, reasoning_efforts: [] });
  if (connector.executor === 'codex') {
    const adapters = connectors.adapters;
    const path = join(sourceRoot('codex', adapters.localHome, adapters.env), 'models_cache.json');
    try {
      if ((await stat(path)).size <= 2 * 1024 * 1024) {
        const cache = JSON.parse(await readFile(path, 'utf8')) as { models?: { slug?: unknown; supported_reasoning_levels?: { effort?: unknown }[] }[] };
        for (const model of Array.isArray(cache.models) ? cache.models : []) if (typeof model.slug === 'string' && model.slug.length < 160 && compatible.test(model.slug)) {
          models.set(model.slug, { id: model.slug, reasoning_efforts: Array.isArray(model.supported_reasoning_levels) ? [...new Set(model.supported_reasoning_levels.flatMap(level => typeof level.effort === 'string' && ['none', 'minimal', 'low', 'medium', 'high', 'xhigh'].includes(level.effort) ? [level.effort] : []))] : [] });
        }
      }
    } catch { /* The native model cache is optional; Default remains available. */ }
  }
  return { connector, models: [...models.values()].slice(0, 500), default_supported: true, timeout_min: 30, timeout_max: 86400 };
}

export async function validateRuntime(connectors: Connectors, org: string, connectorId: unknown, input: unknown) {
  if (typeof connectorId !== 'string') throw new HttpError(422, 'invalid_runtime', '请选择 Connector。');
  const options = await runtimeOptions(connectors, org, connectorId);
  if (!options.connector.enabled || options.connector.status !== 'available') throw new HttpError(409, 'connector_unavailable', '请选择可用的 Connector，或先前往 Connectors 修复连接。');
  if (!input || typeof input !== 'object' || Array.isArray(input)) throw new HttpError(422, 'invalid_runtime', '运行配置无效。');
  const value = input as Record<string, unknown>;
  const model = value.model ?? null; const reasoning = value.reasoning_effort ?? null; const timeout = value.timeout_seconds ?? defaultRuntime.timeout_seconds;
  const selected = options.models.find(item => item.id === model);
  if (model !== null && !selected || reasoning !== null && (!selected || typeof reasoning !== 'string' || !selected.reasoning_efforts.includes(reasoning)) || typeof timeout !== 'number' || !Number.isInteger(timeout) || timeout < options.timeout_min || timeout > options.timeout_max) {
    throw new HttpError(422, 'invalid_runtime', '模型、推理强度或运行超时与当前执行器不兼容，请重新选择。');
  }
  return { connector_id: connectorId, runtime: { model, reasoning_effort: reasoning, timeout_seconds: timeout } as AgentRuntime };
}
