// Preloaded only by the isolated LaunchDaemon fixture's node wrapper. Advancing
// the clock on each awaited delay exercises the real five-second deadline and
// ownership checks without sleeping for it during every rollback scenario.
import { syncBuiltinESMExports } from 'node:module';
import timers from 'node:timers/promises';

let now = Date.now();
const sleep = timers.setTimeout;
Date.now = () => now;
timers.setTimeout = async (milliseconds, value, options) => {
  const result = await sleep(0, value, options);
  now += milliseconds;
  return result;
};
syncBuiltinESMExports();
