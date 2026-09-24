const frame = document.getElementById("workbench");
const phone = document.getElementById("phone-demo");
const base = new URL(frame.getAttribute("src"), location.href);
const feedback = document.getElementById("demo-feedback");
const buttons = [...document.querySelectorAll("[data-scenario]")];
let mode;
const setReady = (ready) => {
  for (const button of buttons) button.disabled = !ready;
};
setReady(false);
new ResizeObserver(([entry]) => {
  const next = entry.contentRect.width <= 820 ? "mobile" : "desktop";
  if (mode === next) return;
  mode = next;
  setReady(false);
  const target = new URL(base);
  target.searchParams.set("ui", mode);
  frame.src = target.href;
}).observe(frame);
for (const button of buttons) {
  button.addEventListener("click", () => {
    frame.contentWindow.postMessage(
      { type: "aow-snapshot-scene", scene: button.dataset.scenario },
      location.origin,
    );
  });
}
document
  .querySelector('[data-action="reset"]')
  .addEventListener("click", () => {
    // Restore the canonical iframe URLs even if the mobile UI changed its route.
    setReady(false);
    frame.src = new URL(`?ui=${mode}`, base).href;
    phone.src = new URL("?ui=mobile", base).href;
    feedback.textContent = "已恢复采集时的数据快照。";
  });
window.addEventListener("message", (event) => {
  if (
    event.origin !== location.origin ||
    event.source !== frame.contentWindow ||
    event.data?.type !== "aow-snapshot-ready"
  )
    return;
  setReady(true);
  feedback.textContent = `AoW ${event.data.revision.slice(0, 7)} · 文件与 Git 数据快照`;
});
