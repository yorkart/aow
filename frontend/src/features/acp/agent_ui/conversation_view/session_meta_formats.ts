import { object } from '../../types';

// Metadata keys define display support independently of the connected Agent.
export const sessionMetaFormats: ReadonlyArray<{ key: string; format: (value: unknown) => string | undefined }> = [
  {
    key: 'trae',
    format: value => {
      const queue = object(object(value).queueStatus);
      if (queue.state === 'ready') return undefined;
      const message = queue.message;
      return typeof message === 'string' && message.trim() ? message : undefined;
    },
  },
];
