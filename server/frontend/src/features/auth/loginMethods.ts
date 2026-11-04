import type { ComponentType } from 'react';
import { PasswordLogin } from './PasswordLogin';
import type { LoginMethodProps } from './types';

// IDs match the server provider registry. Each method owns only its input flow;
// the gate, successful-login state, and API transport remain shared.
export const loginMethods: ReadonlyMap<string, ComponentType<LoginMethodProps>> = new Map([
  ['password', PasswordLogin],
]);
