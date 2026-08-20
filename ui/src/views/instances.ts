import { explain, onDaemonEvent, rpc } from "../api";
import { ago, bytes, clear, h, icons, svg } from "../dom";
import { confirmAction } from "./confirm";
import { openCreateSheet } from "./create";

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

type Column = "name" | "version" | "loader" | "size" | "played";

const active = new Map<string, Progress>();
let sortBy: Column = "played";
let ascending = false;
let filter = "";

export function watchInstalls(rerender: () => void): void {
  onDaemonEvent((event) => {
    if (event.method !== "install.progress") return;
    const id = String(event.params["jobId"] ?? "");
    if (String(event.params["phase"] ?? "") === "done") active.delete(id);
    else
      active.set(id, {
        phase: String(event.params["phase"] ?? ""),
        completedBytes: Number(event.params["completedBytes"] ?? 0),
        totalBytes: Number(event.params["totalBytes"] ?? 0),
        completedFiles: Number(event.params["completedFiles"] ?? 0),
        totalFiles: Number(event.params["totalFiles"] ?? 0),
        reusedBytes: Number(event.params["reusedBytes"] ?? 0),
      });
    rerender();
  });
}

export async function renderInstances(
  toolbar: HTMLElement,
  body: HTMLElement,
  onLaunched: () => void,
): Promise<void> {
  const reload = () => void renderInstances(toolbar, body, onLaunched);
  clear(toolbar);
  clear(body);

  const search = h("input", {
    type: "search",
    placeholder: "Filter",
    value: filter,
    oninput: (event: Event) => {
      filter = (event.target as HTMLInputElement).value;
      reload();
    },
  }) as HTMLInputElement;

  toolbar.appendChild(h("h1", {}, "Instances"));
  toolbar.appendChild(search);
  toolbar.appendChild(h("span", { class: "spacer" }));
  toolbar.appendChild(
    h("button", { class: "btn accent", onclick: () => openCreateSheet(reload) }, svg(icons.plus, 13), "New"),
  );

  let instances: Instance[] = [];
  try {
    instances = (await rpc<{ instances: Instance[] }>("instance.list")).instances ?? [];
  } catch (error) {
    body.appendChild(problem(error));
    return;
  }

  const needle = filter.trim().toLowerCase();
  const shown = instances
    .filter(
      (instance) =>
        !needle ||
        instance.name.toLowerCase().includes(needle) ||
        instance.version.includes(needle) ||
        (instance.loader?.kind ?? "").includes(needle),
    )
    .sort(compare);

  if (shown.length === 0) {
    body.appendChild(
      h(
        "div",
        { class: "blank" },
        h("strong", {}, instances.length === 0 ? "No instances" : "Nothing matches that filter"),
        instances.length === 0 ? "Create one and Acelus fetches and verifies what it needs." : null,
      ),
    );
    return;
  }

  const head = h("tr", {});
  for (const [key, label, right] of [
    ["name", "Instance", false],
    ["version", "Version", false],
    ["loader", "Loader", false],
    ["size", "Disk", true],
    ["played", "Last played", true],
  ] as Array<[Column, string, boolean]>) {
    head.appendChild(
      h(
        "th",
        {
          class: right ? "sortable right" : "sortable",
          style: right ? "text-align:right" : "",
          "aria-sort": sortBy === key ? (ascending ? "ascending" : "descending") : false,
          onclick: () => {
            if (sortBy === key) ascending = !ascending;
            else {
              sortBy = key;
              ascending = key === "name" || key === "version";
            }
            reload();
          },
        },
        label,
      ),
    );
  }
  head.appendChild(h("th", { class: "grow" }));

  const rows = h("tbody", {});
  for (const instance of shown) rows.appendChild(row(instance, reload, onLaunched));

  body.appendChild(h("table", {}, h("thead", {}, head), rows));
}

function compare(a: Instance, b: Instance): number {
  const direction = ascending ? 1 : -1;
  switch (sortBy) {
    case "name":
      return a.name.localeCompare(b.name) * direction;
    case "version":
      return a.version.localeCompare(b.version, undefined, { numeric: true }) * direction;
    case "loader":
      return (a.loader?.kind ?? "").localeCompare(b.loader?.kind ?? "") * direction;
    case "size":
      return ((a.sizeOnDisk ?? 0) - (b.sizeOnDisk ?? 0)) * direction;
    default:
      return ((a.lastPlayed ?? 0) - (b.lastPlayed ?? 0)) * direction;
  }
}

function row(instance: Instance, reload: () => void, onLaunched: () => void): HTMLElement {
  const progress = active.get(instance.id);

  if (progress) {
    const fraction = progress.totalBytes > 0 ? progress.completedBytes / progress.totalBytes : 0;
    return h(
      "tr",
      {},
      h("td", { class: "name" }, instance.name),
      h("td", { class: "data" }, instance.version),
      h(
        "td",
        { colspan: 3 },
        h(
          "div",
          { class: "row" },
          h("span", { class: "bar" }, h("span", { style: `width:${Math.round(fraction * 100)}%` })),
          h(
            "span",
            { class: "data" },
            `${progress.phase} ${progress.completedFiles}/${progress.totalFiles}`,
          ),
          progress.reusedBytes > 0
            ? h("span", { class: "data dim" }, `${bytes(progress.reusedBytes)} reused`)
            : null,
        ),
      ),
      h("td", { class: "actions grow" }),
    );
  }

  const play = h(
    "button",
    {
      class: "btn accent",
      onclick: async (event: Event) => {
        (event.currentTarget as HTMLButtonElement).disabled = true;
        try {
          await rpc("launch.run", { id: instance.id });
          onLaunched();
        } catch (error) {
          reload();
          document.body.appendChild(floatingProblem(error));
        }
      },
    },
    svg(icons.play, 12),
    "Play",
  );

  const install = h(
    "button",
    {
      class: "btn",
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
    svg(icons.download, 12),
    "Install",
  );

  return h(
    "tr",
    {},
    h("td", { class: "name" }, instance.name),
    h("td", { class: "data" }, instance.version),
    h(
      "td",
      {},
      instance.loader
        ? h(
            "span",
            { class: "pill on" },
            `${instance.loader.kind}${instance.loader.version ? ` ${instance.loader.version}` : ""}`,
          )
        : h("span", { class: "data dim" }, "vanilla"),
    ),
    h(
      "td",
      { class: "data right" },
      instance.installed ? (instance.sizeOnDisk ? bytes(instance.sizeOnDisk) : "shared") : "—",
    ),
    h(
      "td",
      { class: "data right dim" },
      instance.installed ? ago(instance.lastPlayed) : "not installed",
    ),
    h(
      "td",
      { class: "actions grow" },
      h(
        "div",
        { class: "row", style: "justify-content:flex-end" },
        h(
          "button",
          {
            class: "btn quiet bad",
            title: "Delete",
            onclick: () =>
              confirmAction({
                title: `Delete ${instance.name}`,
                detail:
                  "Removes the instance directory, including any worlds and screenshots in it. Files shared with other instances stay in the object store.",
                confirmLabel: "Delete",
                destructive: true,
                onConfirm: async () => {
                  await rpc("instance.delete", { id: instance.id, keepUserData: false });
                  reload();
                },
              }),
          },
          svg(icons.trash, 13),
        ),
        instance.installed ? play : install,
      ),
    ),
  );
}

export function problem(error: unknown): HTMLElement {
  const detail = explain(error);
  return h(
    "div",
    { class: "note bad", style: "margin:12px" },
    h("strong", {}, detail.title),
    detail.detail,
    detail.link ? h("div", {}, h("a", { href: detail.link, target: "_blank" }, detail.link)) : null,
  );
}

function floatingProblem(error: unknown): HTMLElement {
  const node = problem(error);
  node.setAttribute("style", "position:fixed;right:14px;bottom:14px;max-width:420px;z-index:50");
  setTimeout(() => node.remove(), 9000);
  return node;
}
