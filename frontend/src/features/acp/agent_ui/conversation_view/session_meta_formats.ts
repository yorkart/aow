import { object } from '../../types';

// Metadata keys define display support independently of the connected Agent.
export const sessionMetaFormats: ReadonlyArray<{ key: string; format: (value: unknown) => string | undefined }> = [
  {
    key: 'trae',
    format: value => {
      const message = object(object(value).queueStatus).message;
      return typeof message === 'string' && message.trim() ? message : undefined;
    },
  },
];
