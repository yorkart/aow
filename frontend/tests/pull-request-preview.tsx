import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import { PullRequestDetailView } from '../src/features/pr/PullRequestDetailView';
import '../src/styles.css';
const scenario = new URLSearchParams(location.search).get('scenario') || 'default';
createRoot(document.getElementById('root')!).render(<StrictMode><PullRequestDetailView repository={'/fixtures/' + scenario} number={42} visible /></StrictMode>);
