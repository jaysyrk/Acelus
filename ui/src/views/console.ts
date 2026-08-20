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

export async function renderConsole(toolbar: HTMLElement, root: HTMLElement): Promise<void> {
  const reload = () => void renderConsole(toolbar, root);
  const wasPinned = pinnedToBottom(root);
  clear(toolbar);
  clear(root);

  let sessions: Session[] = [];
  try {
    const reply = await rpc<{ sessions: Session[] }>("session.list");
    sessions = reply.sessions ?? [];
  } catch {
    sessions = [];
  }

  toolbar.appendChild(h("h1", {}, "Console"));

  if (sessions.length === 0) {
    toolbar.appendChild(h("span", { class: "spacer" }));
    root.appendChild(
      h("div", { class: "blank" }, h("strong", {}, "Nothing running"), "Launch an instance and its output lands here."),
    );
    return;
  }

  const session = sessions[0];
  if (!session) return;

  toolbar.appendChild(h("span", { class: "spin" }));
  toolbar.appendChild(h("span", { class: "data" }, session.instanceId));
  toolbar.appendChild(h("span", { class: "pill" }, `pid ${session.pid}`));
  toolbar.appendChild(h("span", { class: "spacer" }));
  toolbar.appendChild(
    h(
      "button",
      {
        class: "btn quiet",
        onclick: () => {
          buffers.delete(session.sessionId);
          reload();
        },
      },
      svg(icons.refresh, 13),
      "Clear",
    ),
  );
  toolbar.appendChild(
    h(
      "button",
      {
        class: "btn bad",
        onclick: async () => {
          await rpc("session.stop", { sessionId: session.sessionId });
          reload();
        },
      },
      svg(icons.stop, 12),
      "Stop",
    ),
  );

  const pane = h("div", { class: "console" });
  for (const line of buffers.get(session.sessionId) ?? []) {
    pane.appendChild(h("div", { class: /error|exception|caused by/i.test(line) ? "err" : "" }, line));
  }
  root.appendChild(pane);

  if (wasPinned) root.scrollTop = root.scrollHeight;
}

function pinnedToBottom(root: HTMLElement): boolean {
  if (!root.querySelector(".console")) return true;
  return root.scrollHeight - root.scrollTop - root.clientHeight < 60;
}
