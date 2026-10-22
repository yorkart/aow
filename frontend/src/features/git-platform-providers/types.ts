export interface GitPlatformIdentity { provider?: string; remote?: string; provider_name?: string }

export interface GitPlatformTarget { remote: string; host: string; repository: string; provider: string; provider_name: string }

export interface GitPlatformProvider {
  id: string; name: string; enabled: boolean; hosts: string[]; script: string;
}

export interface GitPlatformProviderSettings { revision: number; providers: GitPlatformProvider[] }
