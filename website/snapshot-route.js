// Keep AoW's real Tab URLs refreshable on GitHub Pages without a server router.
const entry = new URL("snapshot/", document.currentScript.src);
if (location.pathname.startsWith(`${entry.pathname}aow/`)) {
  entry.searchParams.set(
    "route",
    location.pathname + location.search + location.hash,
  );
  location.replace(entry.href);
}
