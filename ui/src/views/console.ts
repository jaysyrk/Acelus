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

interface Diagnosis {
  title: string;
  detail: string;
  remedy: { kind: string; containing?: string; megabytes?: number };
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

  const advice = h("div", {});
  root.appendChild(advice);

  const pane = h("div", { class: "console" });
  root.appendChild(pane);

  let shown = "";
  const explainProblem = (found: Diagnosis | null) => {
    if (!found) {
      clear(advice);
      shown = "";
      return;
    }
    if (found.title === shown) return;
    shown = found.title;

    clear(advice);
    const note = h("div", { class: "note bad", style: "margin:12px" }, h("strong", {}, found.title), found.detail);

    if (found.remedy.kind === "removeJvmArgument" && found.remedy.containing) {
      note.appendChild(
        h(
          "div",
          { style: "margin-top:10px" },
          h(
            "button",
            {
              class: "btn accent",
              onclick: async (event: Event) => {
                (event.currentTarget as HTMLButtonElement).disabled = true;
                await rpc("instance.configure", {
                  id: session.instanceId,
                  dropJvmArgumentsContaining: found.remedy.containing,
                });
                await rpc("launch.stop", { sessionId: session.sessionId });
                await rpc("launch.start", { id: session.instanceId });
                reload();
              },
            },
            `Remove it and start again`,
          ),
        ),
      );
    }

    advice.appendChild(note);
  };

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
      const tail = await rpc<{ lines: LogLine[]; diagnosis: Diagnosis | null }>("log.tail", {
        sessionId: session.sessionId,
      });
      draw(tail.lines ?? []);
      explainProblem(tail.diagnosis ?? null);
    } catch {
      stopWatching();
      reload();
    }
  };

  await poll();
  timer = window.setInterval(() => void poll(), POLL_MS);
}
