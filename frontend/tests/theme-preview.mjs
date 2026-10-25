import { themePreviewServer } from './fixtures/theme-preview-server.mjs';

const preview = await themePreviewServer(Number(process.env.THEME_PREVIEW_PORT || 5198));
console.log(`AoW app: ${preview.url}/?ui=desktop`);
console.log(`Theme integration preview: ${preview.url}/tests/theme-preview.html`);
