export interface LoginMethodStatus {
  id: string;
  label: string;
  configured: boolean;
  message?: string;
}

export interface AuthStatus {
  configured: boolean;
  authenticated: boolean;
  message?: string;
  // Older servers and existing embedded fixtures only expose the two flags.
  methods?: LoginMethodStatus[];
}

export interface LoginMethodProps {
  onSuccess: (status: AuthStatus) => void;
}
