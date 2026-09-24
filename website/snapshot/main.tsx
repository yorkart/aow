import { createRoot } from "react-dom/client";
import { useEffect, useState, lazy, Suspense } from "react";
import { appLocalStorage, basePath } from "./basePath";
import { installSnapshotTransport } from "./transport.mjs";
import {
  AowTabEntry,
  useAowTabNavigation,
} from "../../frontend/src/aow/AowTabEntry";
import type {
  ResolvedTab,
  TabTarget,
} from "../../frontend/src/aow/tabRoutes/types";
import "../../frontend/src/styles.css";
import "./snapshot.css";

const snapshot = await fetch(new URL("data.json", document.baseURI)).then(
  (response) => {
    if (!response.ok) throw new Error("无法读取工作台数据快照");
    return response.json();
  },
);
installSnapshotTransport(snapshot);
// GitHub Pages sends deep links through 404.html to this canonical entry.
const restoredRoute = new URLSearchParams(location.search).get("route");
if (restoredRoute) {
  const url = new URL(restoredRoute, location.origin);
  if (
    url.origin === location.origin &&
    url.pathname.startsWith(`${basePath}/aow/`)
  ) {
    history.replaceState(null, "", url.pathname + url.search + url.hash);
  }
}
const project = snapshot.responses["/api/aow/projects"][0];
const worktree = project.worktrees.find(
  (item: { is_main: boolean }) => item.is_main,
);
const mode = new URLSearchParams(location.search).get("ui");
const mobile =
  mode === "mobile" ||
  (mode !== "desktop" && matchMedia("(max-width: 820px)").matches);
const View = mobile
  ? lazy(() =>
      import("../../frontend/src/mobile/MobileAow").then((module) => ({
        default: module.MobileAow,
      })),
    )
  : lazy(() =>
      import("../../frontend/src/aow/ProjectAow").then((module) => ({
        default: module.ProjectAow,
      })),
    );
const readme = `${worktree.path}/README.md`;
appLocalStorage.setItem(
  `aow-workspace-tabs:${worktree.path}`,
  JSON.stringify({
    documents: [
      {
        id: readme,
        path: readme,
        name: "README.md",
        kind: "markdown",
        readOnly: true,
        explorerSource: "project",
      },
    ],
    active: "document:" + readme,
  }),
);

function SnapshotView({ entry }: { entry?: ResolvedTab }) {
  const navigate = useAowTabNavigation();
  useEffect(() => {
    let panelObserver: MutationObserver | undefined;
    const ready = new MutationObserver(() => {
      if (
        !document.querySelector(
          ".project-aow-surface, .mobile-project-card, .mobile-workspace",
        )
      )
        return;
      ready.disconnect();
      window.parent.postMessage(
        {
          type: "aow-snapshot-ready",
          revision: snapshot.revision,
          capturedAt: snapshot.capturedAt,
        },
        location.origin,
      );
    });
    ready.observe(document.getElementById("root")!, {
      childList: true,
      subtree: true,
    });
    const receive = async (event: MessageEvent) => {
      if (
        event.source !== window.parent ||
        event.origin !== location.origin ||
        event.data?.type !== "aow-snapshot-scene"
      )
        return;
      if (!["workspace", "terminal", "git"].includes(event.data.scene)) return;
      panelObserver?.disconnect();
      if (event.data.scene === "git") {
        const open = () => {
          const button = document.querySelector<HTMLButtonElement>(
            mobile
              ? 'button[aria-label="Git"]'
              : '.project-aow-surface:not([hidden]) button[aria-label="Source Control"]',
          );
          if (!button) return false;
          button.click();
          panelObserver?.disconnect();
          return true;
        };
        if (open()) return;
        panelObserver = new MutationObserver(open);
        panelObserver.observe(document.getElementById("root")!, {
          childList: true,
          subtree: true,
        });
        if (mobile)
          await navigate({ type: "terminal", tabId: snapshot.terminal.tabId });
        return;
      }
      const target: TabTarget =
        event.data.scene === "terminal"
          ? { type: "terminal", tabId: snapshot.terminal.tabId }
          : {
              type: "file",
              workspace: worktree.id,
              path: readme,
              source: "project",
            };
      try {
        await navigate(target);
      } catch (error) {
        window.dispatchEvent(
          new CustomEvent("snapshot-notice", { detail: String(error) }),
        );
      }
    };
    // Relative links in the captured Markdown should open another real file Tab.
    const openDocumentLink = (event: MouseEvent) => {
      const anchor = (event.target as Element).closest<HTMLAnchorElement>(
        ".markdown-preview a, .mobile-markdown a",
      );
      const href = anchor?.getAttribute("href");
      if (!href || /^(?:[a-z][a-z\d+.-]*:|\/\/|#)/i.test(href)) return;
      const currentPath = new URLSearchParams(location.search).get("path");
      if (!currentPath) return;
      event.preventDefault();
      const path = decodeURI(
        new URL(href, `https://snapshot${currentPath}`).pathname,
      );
      if (!Object.hasOwn(snapshot.responses, `/api/fs/text${path}`)) {
        window.dispatchEvent(
          new CustomEvent("snapshot-notice", {
            detail: "这份文件未收录在当前快照中。",
          }),
        );
        return;
      }
      void navigate({
        type: "file",
        workspace: worktree.id,
        path,
        source: "project",
      }).catch((error) => {
        window.dispatchEvent(
          new CustomEvent("snapshot-notice", { detail: String(error) }),
        );
      });
    };
    window.addEventListener("message", receive);
    document.addEventListener("click", openDocumentLink);
    return () => {
      ready.disconnect();
      panelObserver?.disconnect();
      window.removeEventListener("message", receive);
      document.removeEventListener("click", openDocumentLink);
    };
  }, [navigate]);
  return <View initialEntry={entry} />;
}
function SnapshotApp() {
  const [notice, setNotice] = useState("");
  useEffect(() => {
    const explain = (event: Event) =>
      setNotice((event as CustomEvent<string>).detail);
    window.addEventListener("snapshot-notice", explain);
    return () => window.removeEventListener("snapshot-notice", explain);
  }, []);
  useEffect(() => {
    if (notice) {
      const timer = setTimeout(() => setNotice(""), 5000);
      return () => clearTimeout(timer);
    }
  }, [notice]);
  return (
    <>
      <div className="snapshot-strip">
        <span title={`采集于 ${snapshot.capturedAt}`}>
          数据快照 · {snapshot.revision.slice(0, 7)}
        </span>
        <span>浏览与切换 · 只读</span>
      </div>
      <div className="snapshot-app">
        <AowTabEntry>
          {(entry) => (
            <Suspense
              fallback={<p className="snapshot-loading">正在打开 AoW…</p>}
            >
              <SnapshotView entry={entry} />
            </Suspense>
          )}
        </AowTabEntry>
      </div>
      {notice && (
        <p className="snapshot-notice" role="status">
          {notice}
        </p>
      )}
    </>
  );
}
createRoot(document.getElementById("root")!).render(<SnapshotApp />);
