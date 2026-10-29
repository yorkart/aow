import { configuration } from './config.js';
import { openDatabase } from './database.js';
import { createOrbitServer } from './http.js';

const config = configuration();
const database = openDatabase(config.stateDir);
const server = createOrbitServer(config, database);
server.on('error', error => {
  console.error(`Orbit 无法启动：${error.message}`);
  database.close();
  process.exitCode = 1;
});
server.listen(config.port, config.host, () => {
  const address = server.address();
  if (address && typeof address !== 'string') console.log(`Orbit: http://${config.host.includes(':') ? `[${config.host}]` : config.host}:${address.port}`);
});
let closing = false;
function shutdown() {
  if (closing) return;
  closing = true;
  server.close(() => database.close());
  server.closeIdleConnections();
}
process.on('SIGINT', shutdown);
process.on('SIGTERM', shutdown);
