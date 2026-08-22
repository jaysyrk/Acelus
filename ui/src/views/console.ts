import { rpc } from "../api";
import { clear, h, icons, svg } from "../dom";

interface Session {
  sessionId: string;
  instanceId: string;
  pid: number;
}

interface LogLine {
  stream: string;
  line: string;
}

const POLL_MS = 800;

let timer: number | null = null;

export function watchLogs(): void {
  return;
}

export function stopWatching(): void {
  if (timer !== null) {
    window.clearInterval(timer);
    timer = null;
  }
}

export async function renderConsole(toolbar: HTMLElement, root: HTMLElement): Promise<void> {
  stopWatching();
  clear(toolbar);
  clear(root);

  toolbar.appendChild(h("h1", {}, "Console"));

  let sessions: Session[] = [];
  try {
    sessions = (await rpc<{ sessions: Session[] }>("launch.status")).sessions ?? [];
  } catch {
    sessions = [];
  }

  const session = sessions[0];

  if (!session) {
    toolbar.appendChild(h("span", { class: "spacer" }));
    root.appendChild(
      h(
        "div",
        { class: "blank" },
        h("strong", {}, "Nothing running"),
        "Press Play on an instance and its output lands here.",
      ),
    );
    return;
  }

  const reload = () => void renderConsole(toolbar, root);

  toolbar.appendChild(h("span", { class: "spin" }));
  toolbar.appendChild(h("span", { class: "data" }, session.instanceId));
  toolbar.appendChild(h("span", { class: "pill" }, `pid ${session.pid}`));
  toolbar.appendChild(h("span", { class: "spacer" }));
  toolbar.appendChild(
    h(
      "button",
      {
        class: "btn bad",
        onclick: async () => {
          stopWatching();
          await rpc("launch.stop", { sessionId: session.sessionId });
          reload();
        },
      },
      svg(icons.stop, 12),
      "Stop",
    ),
  );

  const pane = h("div", { class: "console" });
  root.appendChild(pane);

  const draw = (lines: LogLine[]) => {
    const pinned = root.scrollHeight - root.scrollTop - root.clientHeight < 60;
    clear(pane);
    for (const held of lines) {
      const bad = held.stream === "stderr" || /error|exception|caused by/i.test(held.line);
      pane.appendChild(h("div", { class: bad ? "err" : "" }, held.line));
    }
    if (pinned) root.scrollTop = root.scrollHeight;
  };

  const poll = async () => {
    try {
      const tail = await rpc<{ lines: LogLine[] }>("log.tail", {
        sessionId: session.sessionId,
      });
      draw(tail.lines ?? []);
    } catch {
      stopWatching();
      reload();
    }
  };

  await poll();
  timer = window.setInterval(() => void poll(), POLL_MS);
}
