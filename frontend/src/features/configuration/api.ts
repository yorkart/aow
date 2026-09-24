import { aowRequest } from '../../lib/aowRequest';
import type { ConfigSelection, ConfigurationSettings, RepositoryVersions } from './types';

const endpoint = '/api/aow/settings/configuration';

export const configurationApi = {
  settings: () => aowRequest<ConfigurationSettings>(endpoint, { cache: 'no-store' }),
  versions: (path: string) => aowRequest<RepositoryVersions>(`${endpoint}/versions?${new URLSearchParams({ path })}`, { cache: 'no-store' }),
  save: (selection: ConfigSelection) => aowRequest<ConfigurationSettings>(endpoint, { method: 'PUT', body: JSON.stringify(selection) }),
};
