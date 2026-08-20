import { explain, onDaemonEvent, openExternal, rpc } from "../api";
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
  contentSize?: number;
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
let known: Instance[] = [];
let accountsReady = true;
let repaint: (() => void) | null = null;
let goAccounts: (() => void) | null = null;
let refresh: (() => void) | null = null;
let pending = false;

export function whenAccountsNeeded(open: () => void): void {
  goAccounts = open;
}

export function watchInstalls(): void {
  onDaemonEvent((event) => {
    if (event.method !== "install.progress") return;
    const id = String(event.params["jobId"] ?? "");
    const phase = String(event.params["phase"] ?? "");
    if (phase === "done") {
      active.delete(id);
      refresh?.();
      return;
    }
    active.set(id, {
      phase,
      completedBytes: Number(event.params["completedBytes"] ?? 0),
      totalBytes: Number(event.params["totalBytes"] ?? 0),
      completedFiles: Number(event.params["completedFiles"] ?? 0),
      totalFiles: Number(event.params["totalFiles"] ?? 0),
      reusedBytes: Number(event.params["reusedBytes"] ?? 0),
    });
    schedule();
  });
}

function schedule(): void {
  if (pending || !repaint) return;
  pending = true;
  requestAnimationFrame(() => {
    pending = false;
    repaint?.();
  });
}

export async function renderInstances(
  toolbar: HTMLElement,
  body: HTMLElement,
  onLaunched: () => void,
): Promise<void> {
  const refetch = () => void renderInstances(toolbar, body, onLaunched);
  clear(toolbar);

  const search = h("input", {
    type: "search",
    placeholder: "Filter",
    value: filter,
    oninput: (event: Event) => {
      filter = (event.target as HTMLInputElement).value;
      paint();
    },
  }) as HTMLInputElement;

  toolbar.appendChild(h("h1", {}, "Instances"));
  toolbar.appendChild(search);
  toolbar.appendChild(h("span", { class: "spacer" }));
  toolbar.appendChild(
    h("button", { class: "btn accent", onclick: () => openCreateSheet(refetch) }, svg(icons.plus, 13), "New"),
  );

  try {
    known = (await rpc<{ instances: Instance[] }>("instance.list")).instances ?? [];
  } catch (error) {
    clear(body);
    body.appendChild(problem(error));
    return;
  }

  try {
    const reply = await rpc<{ accounts: unknown[] }>("account.list");
    accountsReady = (reply.accounts ?? []).length > 0;
  } catch {
    accountsReady = true;
  }

  const paint = () => draw(body, refetch, onLaunched);
  repaint = paint;
  refresh = refetch;
  paint();
}

function draw(body: HTMLElement, refetch: () => void, onLaunched: () => void): void {
  const instances = known;
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

  const table = h("div", {});

  if (!accountsReady && instances.length > 0) {
    table.appendChild(
      h(
        "div",
        { class: "note measure", style: "margin:12px" },
        h("strong", {}, "Sign in before you play"),
        "Acelus checks that your Microsoft account owns Minecraft before it starts the game. ",
        h(
          "button",
          { class: "btn", style: "margin-top:8px", onclick: () => goAccounts?.() },
          "Go to Accounts",
        ),
      ),
    );
  }

  if (shown.length === 0) {
    table.appendChild(
      h(
        "div",
        { class: "blank" },
        h("strong", {}, instances.length === 0 ? "No instances" : "Nothing matches that filter"),
        instances.length === 0
          ? "Create one with New, and Acelus downloads and checks everything it needs."
          : null,
      ),
    );
    swap(body, table);
    return;
  }

  const head = h("tr", {});
  for (const [key, label, right] of [
    ["name", "Instance", false],
    ["version", "Version", false],
    ["loader", "Loader", false],
    ["size", "Size", true],
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
            draw(body, refetch, onLaunched);
          },
        },
        label,
      ),
    );
  }
  head.appendChild(h("th", { class: "grow" }));

  const rows = h("tbody", {});
  for (const instance of shown) rows.appendChild(row(instance, refetch, onLaunched));

  table.appendChild(h("table", {}, h("thead", {}, head), rows));
  swap(body, table);
}

function swap(body: HTMLElement, next: HTMLElement): void {
  const top = body.scrollTop;
  clear(body);
  body.appendChild(next);
  body.scrollTop = top;
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
      return ((a.contentSize ?? 0) - (b.contentSize ?? 0)) * direction;
    default:
      return ((a.lastPlayed ?? 0) - (b.lastPlayed ?? 0)) * direction;
  }
}

function row(instance: Instance, refetch: () => void, onLaunched: () => void): HTMLElement {
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
        if (!accountsReady) {
          goAccounts?.();
          return;
        }
        (event.currentTarget as HTMLButtonElement).disabled = true;
        try {
          await rpc("launch.run", { id: instance.id });
          onLaunched();
        } catch (error) {
          refetch();
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
      instance.installed && instance.contentSize ? bytes(instance.contentSize) : "—",
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
                  refetch();
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
    detail.link
      ? h(
          "div",
          { style: "margin-top:8px" },
          h(
            "button",
            { class: "btn", onclick: () => void openExternal(detail.link as string) },
            svg(icons.external, 13),
            "Open",
          ),
        )
      : null,
  );
}

function floatingProblem(error: unknown): HTMLElement {
  const node = problem(error);
  node.setAttribute("style", "position:fixed;right:14px;bottom:14px;max-width:420px;z-index:50");
  setTimeout(() => node.remove(), 9000);
  return node;
}
