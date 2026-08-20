import { onDaemonEvent, rpc } from "../api";
import { clear, h, icons, svg } from "../dom";

interface Session {
  sessionId: string;
  instanceId: string;
  pid: number;
}

const buffers = new Map<string, string[]>();
const LIMIT = 4000;

export function watchLogs(rerender: () => void): void {
  onDaemonEvent((event) => {
    if (event.method === "log.line") {
      const id = String(event.params["sessionId"] ?? "");
      const lines = buffers.get(id) ?? [];
      lines.push(String(event.params["line"] ?? ""));
      if (lines.length > LIMIT) lines.splice(0, lines.length - LIMIT);
      buffers.set(id, lines);
      rerender();
    }
    if (event.method === "session.ended") {
      rerender();
    }
  });
}

export async function renderConsole(root: HTMLElement): Promise<void> {
  const reload = () => void renderConsole(root);
  const wasPinned = pinnedToBottom(root);
  clear(root);

  let sessions: Session[] = [];
  try {
    const reply = await rpc<{ sessions: Session[] }>("session.list");
    sessions = reply.sessions ?? [];
  } catch {
    sessions = [];
  }

  root.appendChild(
    h(
      "div",
      { class: "page-head" },
      h(
        "div",
        {},
        h("h1", {}, "Console"),
        h("p", { class: "page-sub" }, "The game's output, captured while it runs."),
      ),
    ),
  );

  if (sessions.length === 0) {
    root.appendChild(
      h(
        "div",
        { class: "empty" },
        h("h2", {}, "Nothing is running"),
        h("p", {}, "Launch an instance and its output will stream here."),
      ),
    );
    return;
  }

  for (const session of sessions) {
    root.appendChild(
      h(
        "div",
        { class: "session-bar" },
        h("span", { class: "spin" }),
        h("strong", {}, session.instanceId),
        h("span", { class: "tag" }, `pid ${session.pid}`),
        h("span", { style: "flex:1" }),
        h(
          "button",
          {
            class: "btn ghost",
            onclick: () => {
              buffers.delete(session.sessionId);
              reload();
            },
          },
          svg(icons.refresh, 15),
          "Clear",
        ),
        h(
          "button",
          {
            class: "btn danger",
            onclick: async () => {
              await rpc("session.stop", { sessionId: session.sessionId });
              reload();
            },
          },
          svg(icons.stop, 14),
          "Stop",
        ),
      ),
    );

    const pane = h("div", { class: "console" });
    for (const line of buffers.get(session.sessionId) ?? []) {
      pane.appendChild(h("div", { class: /error|exception|caused by/i.test(line) ? "err" : "" }, line));
    }
    root.appendChild(pane);

    if (wasPinned) pane.scrollTop = pane.scrollHeight;
  }
}

function pinnedToBottom(root: HTMLElement): boolean {
  const pane = root.querySelector(".console");
  if (!pane) return true;
  return pane.scrollHeight - pane.scrollTop - pane.clientHeight < 60;
}
