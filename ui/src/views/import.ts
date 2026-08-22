import { explain, rpc } from "../api";
import { clear, h, icons, svg } from "../dom";

interface Foreign {
  path: string;
  name: string;
  version: string;
  blockedBy?: string | null;
  memoryMegabytes?: number | null;
  loader?: { kind: string; version?: string } | null;
}

export async function countImportable(): Promise<number> {
  try {
    const found = await rpc<{ found: Foreign[] }>("import.scan");
    return (found.found ?? []).filter((one) => !one.blockedBy).length;
  } catch {
    return 0;
  }
}

export function openImportSheet(onImported: () => void): void {
  const scrim = h("div", { class: "scrim" });
  const body = h("div", { class: "sheet-body" });
  const close = () => {
    scrim.remove();
    onImported();
  };

  scrim.appendChild(
    h(
      "div",
      { class: "sheet", style: "width:min(560px,100%)" },
      h("div", { class: "sheet-head" }, "Bring instances over"),
      body,
      h("div", { class: "sheet-foot" }, h("button", { class: "btn", onclick: close }, "Done")),
    ),
  );

  scrim.addEventListener("click", (event) => {
    if (event.target === scrim) close();
  });
  document.body.appendChild(scrim);

  body.appendChild(h("div", { class: "row" }, h("span", { class: "spin" }), "Looking for other launchers"));

  void (async () => {
    let found: Foreign[] = [];
    try {
      found = (await rpc<{ found: Foreign[] }>("import.scan")).found ?? [];
    } catch (error) {
      clear(body);
      body.appendChild(failure(error));
      return;
    }

    clear(body);

    if (found.length === 0) {
      body.appendChild(
        h(
          "div",
          { class: "blank", style: "padding:26px 8px" },
          h("strong", {}, "Nothing found"),
          "Acelus looked where Prism, PolyMC and MultiMC keep their instances.",
        ),
      );
      return;
    }

    body.appendChild(
      h(
        "p",
        { style: "margin:0;color:var(--muted)" },
        "Your worlds, mods and settings are copied. The original stays where it is, so the other launcher keeps working.",
      ),
    );

    for (const one of found) body.appendChild(row(one, onImported));
  })();
}

function row(one: Foreign, onImported: () => void): HTMLElement {
  const status = h("span", { class: "data dim" });

  const action = one.blockedBy
    ? h("span", { class: "pill" }, `${one.blockedBy} not supported yet`)
    : h(
        "button",
        {
          class: "btn accent",
          onclick: async (event: Event) => {
            const button = event.currentTarget as HTMLButtonElement;
            button.disabled = true;
            status.textContent = "copying";

            try {
              const done = await rpc<{ instance: { id: string }; copiedBytes: number }>(
                "import.run",
                { path: one.path },
              );
              status.textContent = "installing";
              onImported();
              await rpc("install.run", { id: done.instance.id });
              status.textContent = "done";
              button.replaceWith(h("span", { class: "pill on" }, "imported"));
              onImported();
            } catch (error) {
              button.disabled = false;
              status.textContent = "";
              (button.parentElement ?? document.body).appendChild(failure(error));
            }
          },
        },
        svg(icons.download, 13),
        "Import",
      );

  return h(
    "div",
    { class: "account" },
    h(
      "div",
      { style: "flex:1;min-width:0" },
      h("div", { style: "font-weight:550" }, one.name),
      h(
        "div",
        { class: "data dim", style: "font-size:11px" },
        `${one.version} · ${describeLoader(one)}`,
      ),
    ),
    status,
    action,
  );
}

function describeLoader(one: Foreign): string {
  if (one.loader) return `${one.loader.kind} ${one.loader.version ?? ""}`.trimEnd();
  if (one.blockedBy) return one.blockedBy.toLowerCase();
  return "vanilla";
}

function failure(error: unknown): HTMLElement {
  const detail = explain(error);
  return h("div", { class: "note bad", style: "margin-top:8px" }, h("strong", {}, detail.title), detail.detail);
}
