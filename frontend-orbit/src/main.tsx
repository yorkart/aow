import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { App } from './App';
import { LoginGate } from './features/auth/LoginGate';
import { initializeTheme } from './lib/theme/store';
import './styles.css';

initializeTheme();
createRoot(document.getElementById('root')!).render(<StrictMode><LoginGate><App /></LoginGate></StrictMode>);
