import { createContext, useContext } from 'react';
import type { TabTarget } from './tabRoutes/types';

// Keep consumers independent of the route registry and the panels it can open.
export const TabNavigationContext = createContext<(target: TabTarget) => Promise<void>>(async () => {});
export const useAowTabNavigation = () => useContext(TabNavigationContext);
