import { createContext, useContext } from 'react';

export const ListRowMenuContext = createContext<{
  target?: HTMLElement;
  open: (element: HTMLElement, x: number, y: number) => void;
} | null>(null);

export const useListRowMenu = () => useContext(ListRowMenuContext);
