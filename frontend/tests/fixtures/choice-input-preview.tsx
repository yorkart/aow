import { createRoot } from 'react-dom/client';
import { useEffect, useRef } from 'react';
import { initializeTheme } from '../../src/lib/theme/store';
import '../../src/styles.css';
import '../../src/features/workspaces/workspaces.css';
import '../../src/features/configuration/configuration-settings.css';
import '../../src/features/settings/appearance-settings.css';
import '../../src/features/automations/automations.css';
import '../../src/features/notifications/notification-settings.css';
import '../../src/mobile/mobile-settings.css';

initializeTheme();
function Preview() {
  const mixed = useRef<HTMLInputElement>(null);
  useEffect(() => { if (mixed.current) mixed.current.indeterminate = true; }, []);
  const mobile = new URLSearchParams(location.search).has('mobile');
  return <section className={`project-aow-dialog${mobile ? ' mobile-settings' : ''}`} style={{ padding: 16 }}>
    <form onSubmit={event => { event.preventDefault(); document.body.dataset.submitted = 'true'; }}>
      <fieldset><legend>Workspace and configuration radios</legend>
        <div className="workspace-field"><div className="workspace-dynamic-options"><label><input type="radio" name="workspace" value="temporary" required />Temporary workspace</label></div></div>
        <label className="configuration-version"><input type="radio" name="workspace" value="dynamic" />Dynamic workspace</label>
        <label><input type="radio" name="workspace" value="disabled" disabled />Unavailable workspace</label>
      </fieldset>
      <fieldset className="automation-settings"><legend>Checkbox states</legend>
        <label className="automation-yolo-option"><input type="checkbox" name="enabled" />Enable automation</label>
        <label className="automation-checkbox"><input type="checkbox" defaultChecked />Checked setting</label>
        <label className="project-aow-dialog-checkbox"><input ref={mixed} type="checkbox" />Mixed setting</label>
        <label><input type="checkbox" disabled />Disabled setting</label>
        <label><input type="checkbox" disabled defaultChecked />Disabled checked setting</label>
      </fieldset>
      <button className="project-aow-dialog-button primary" type="submit">Save choices</button>
    </form>
    <fieldset disabled><legend>Disabled group</legend><label><input type="checkbox" />Inherited disabled setting</label></fieldset>
    <label className="theme-card" style={{ width: 220, marginTop: 16 }}><input type="radio" name="theme-card" defaultChecked /><span className="theme-preview">Theme card</span></label>
  </section>;
}
createRoot(document.getElementById('root')!).render(<Preview />);
