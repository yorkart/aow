import { ProjectAow } from './ProjectAow';
import type { ResolvedTab } from './tabRoutes';
import '../styles.css';

export default function Desktop({ initialEntry }: { initialEntry?: ResolvedTab }) {
  return <ProjectAow initialEntry={initialEntry} />;
}
