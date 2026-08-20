import { explain, onDaemonEvent, rpc, type Json } from "../api";
import { ago, bytes, clear, h, icons, svg } from "../dom";
import { confirmAction } from "./confirm";
import { openCreateDialog } from "./create";

interface Loader {
  kind: string;
  version?: string;
}

export interface Instance {
  id: string;
  name: string;
  version: string;
  loader?: Loader | null;
  path: string;
  installed: boolean;
  lastPlayed: number | null;
  sizeOnDisk?: number;
}

interface Progress {
  phase: string;
  completedBytes: number;
  totalBytes: number;
  completedFiles: number;
  totalFiles: number;
  reusedBytes: number;
}

const active = new Map<string, Progress>();

export function watchInstalls(rerender: () => void): void {
  onDaemonEvent((event) => {
    if (event.method !== "install.progress") return;
    const id = String(event.params["jobId"] ?? "");
    const phase = String(event.params["phase"] ?? "");

    if (phase === "done") {
      active.delete(id);
    } else {
      active.set(id, {
        phase,
        completedBytes: Number(event.params["completedBytes"] ?? 0),
        totalBytes: Number(event.params["totalBytes"] ?? 0),
        completedFiles: Number(event.params["completedFiles"] ?? 0),
        totalFiles: Number(event.params["totalFiles"] ?? 0),
        reusedBytes: Number(event.params["reusedBytes"] ?? 0),
      });
    }
    rerender();
  });
}

export async function renderInstances(root: HTMLElement, onLaunched: (id: string) => void): Promise<void> {
  const reload = () => void renderInstances(root, onLaunched);
  clear(root);

  const head = h(
    "div",
    { class: "page-head" },
    h(
      "div",
      {},
      h("h1", {}, "Instances"),
      h("p", { class: "page-sub" }, "Every instance is a real directory you can open and read."),
    ),
    h(
      "button",
      { class: "btn primary", onclick: () => openCreateDialog(reload) },
      svg(icons.plus, 15),
      "New instance",
    ),
  );
  root.appendChild(head);

  let instances: Instance[] = [];
  try {
    const reply = await rpc<{ instances: Instance[] }>("instance.list");
    instances = reply.instances ?? [];
  } catch (error) {
    root.appendChild(problem(error));
    return;
  }

  if (instances.length === 0) {
    root.appendChild(
      h(
        "div",
        { class: "empty" },
        h("h2", {}, "No instances yet"),
        h("p", {}, "Create one and Acelus will fetch and verify everything it needs."),
        h(
          "button",
          { class: "btn primary", onclick: () => openCreateDialog(reload) },
          svg(icons.plus, 15),
          "New instance",
        ),
      ),
    );
    return;
  }

  const grid = h("div", { class: "grid" });
  for (const instance of instances) grid.appendChild(card(instance, reload, onLaunched));
  root.appendChild(grid);
}

function card(instance: Instance, reload: () => void, onLaunched: (id: string) => void): HTMLElement {
  const progress = active.get(instance.id);

  const tags = h(
    "div",
    { class: "meta" },
    h("span", { class: "tag" }, instance.version),
    instance.loader
      ? h("span", { class: "tag loader" }, `${instance.loader.kind} ${instance.loader.version ?? ""}`.trim())
      : null,
    instance.installed
      ? h(
          "span",
          { class: "tag", title: "Bytes unique to this instance, excluding anything shared" },
          instance.sizeOnDisk ? bytes(instance.sizeOnDisk) : "shared",
        )
      : null,
  );

  const body = h(
    "div",
    { class: "card" },
    h(
      "div",
      { class: "card-top" },
      h("div", {}, h("h3", { class: "card-name" }, instance.name), tags),
    ),
  );

  if (progress) {
    const fraction = progress.totalBytes > 0 ? progress.completedBytes / progress.totalBytes : 0;
    body.appendChild(
      h("div", { class: "progress" }, h("span", { style: `width:${Math.round(fraction * 100)}%` })),
    );
    body.appendChild(
      h(
        "div",
        { class: "progress-detail" },
        h("span", {}, `${progress.phase} ${progress.completedFiles}/${progress.totalFiles}`),
        h(
          "span",
          {},
          progress.reusedBytes > 0
            ? `${bytes(progress.reusedBytes)} reused`
            : bytes(progress.completedBytes),
        ),
      ),
    );
    return body;
  }

  const state = h(
    "span",
    { class: "state" },
    instance.installed ? ago(instance.lastPlayed) : "not installed yet",
  );

  const action = instance.installed
    ? h(
        "button",
        {
          class: "btn primary",
          onclick: async (event: Event) => {
            const button = event.currentTarget as HTMLButtonElement;
            button.disabled = true;
            try {
              await rpc("launch.run", { id: instance.id });
              onLaunched(instance.id);
            } catch (error) {
              button.disabled = false;
              body.appendChild(problem(error));
            }
          },
        },
        svg(icons.play, 14),
        "Play",
      )
    : h(
        "button",
        {
          class: "btn primary",
          onclick: async (event: Event) => {
            (event.currentTarget as HTMLButtonElement).disabled = true;
            active.set(instance.id, {
              phase: "resolve",
              completedBytes: 0,
              totalBytes: 0,
              completedFiles: 0,
              totalFiles: 0,
              reusedBytes: 0,
            });
            await rpc("install.run", { id: instance.id });
          },
        },
        svg(icons.download, 14),
        "Install",
      );

  body.appendChild(
    h(
      "div",
      { class: "card-foot" },
      state,
      h(
        "div",
        { class: "row" },
        h(
          "button",
          {
            class: "btn ghost danger",
            title: "Delete this instance",
            onclick: () =>
              confirmAction({
                title: `Delete ${instance.name}?`,
                detail:
                  "The instance directory is removed, including worlds and screenshots inside it. Files shared with other instances stay in the object store.",
                confirmLabel: "Delete instance",
                destructive: true,
                onConfirm: async () => {
                  await rpc("instance.delete", { id: instance.id, keepUserData: false });
                  reload();
                },
              }),
          },
          svg(icons.trash, 15),
        ),
        action,
      ),
    ),
  );

  return body;
}

export function problem(error: unknown): HTMLElement {
  const detail = explain(error);
  return h(
    "div",
    { class: "notice bad" },
    h("strong", {}, detail.title),
    detail.detail,
    detail.link ? h("div", {}, h("a", { href: detail.link, target: "_blank" }, detail.link)) : null,
  );
}

export function instanceJson(instance: Instance): Json {
  return instance as unknown as Json;
}
