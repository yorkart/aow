import { createAssistantMessageEventStream } from '@earendil-works/pi-ai/compat';

// Offline provider for integration tests. Never contacts a model endpoint.
export default function fixture(pi) {
  pi.registerProvider('aow-test', {
    baseUrl: 'https://invalid.example',
    apiKey: 'fixture-only',
    api: 'openai-completions',
    models: [{ id: 'fixture', name: 'Fixture', reasoning: false, input: ['text'],
      cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0 }, contextWindow: 200000, maxTokens: 1024 }],
    streamSimple(model) {
      const stream = createAssistantMessageEventStream();
      const failed = process.env.AOW_PI_TEST_FAILURE === '1';
      const message = {
        role: 'assistant', api: model.api, provider: model.provider, model: model.id,
        content: failed ? [] : [{ type: 'text', text: 'Pi fixture reply' }],
        stopReason: failed ? 'error' : 'stop', timestamp: Date.now(),
        usage: { input: 10, output: 5, cacheRead: 3, cacheWrite: 2, totalTokens: 20,
          cost: { input: 0, output: 0, cacheRead: 0, cacheWrite: 0, total: 0 } },
        ...(failed ? { errorMessage: 'fixture failure' } : {}),
      };
      stream.push(failed ? { type: 'error', reason: 'error', error: message } : { type: 'done', reason: 'stop', message });
      stream.end(message);
      return stream;
    },
  });
}
