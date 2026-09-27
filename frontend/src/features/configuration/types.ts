export interface ConfigSelection {
  config_repo: string;
  config_id: string;
}

export interface ConfigurationSettings {
  selection: ConfigSelection;
  active_selection: ConfigSelection;
  restart_required: boolean;
  changed: boolean;
}

export interface RepositoryVersions {
  config_repo: string;
  config_ids: string[];
  selected_id: string | null;
}
