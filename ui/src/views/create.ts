import { rpc } from "../api";
import { clear, h, icons, svg } from "../dom";

interface Version {
  id: string;
  type: string;
  releaseTime: string;
}

interface LoaderBuild {
  version: string;
  stable: boolean;
}

type LoaderChoice = "none" | "fabric" | "quilt";

export function openCreateDialog(onCreated: () => void): void {
  let versions: Version[] = [];
  let chosenVersion = "";
  let loader: LoaderChoice = "none";
  let loaderBuilds: LoaderBuild[] = [];
  let chosenBuild = "";
  let showSnapshots = false;
  let filter = "";
  let busy = false;

  const scrim = h("div", { class: "scrim" });
  const list = h("div", { class: "picker-list" });
  const loaderSlot = h("div", {});
  const footNote = h("div", { style: "flex:1" });

  const nameInput = h("input", {
    type: "text",
    placeholder: "Survival",
    oninput: () => refreshFooter(),
  }) as HTMLInputElement;

  const search = h("input", {
    type: "search",
    placeholder: "Filter versions",
    oninput: (event: Event) => {
      filter = (event.target as HTMLInputElement).value.toLowerCase();
      renderVersions();
    },
  }) as HTMLInputElement;

  const createButton = h("button", { class: "btn primary", onclick: () => void create() }, "Create");

  function refreshFooter(): void {
    const ready = nameInput.value.trim().length > 0 && chosenVersion.length > 0 && !busy;
    (createButton as HTMLButtonElement).disabled = !ready;
  }

  function renderVersions(): void {
    clear(list);
    const shown = versions
      .filter((version) => showSnapshots || version.type === "release")
      .filter((version) => version.id.toLowerCase().includes(filter))
      .slice(0, 120);

    for (const version of shown) {
      const row = h(
        "button",
        {
          class: "picker-row",
          "aria-selected": version.id === chosenVersion,
          onclick: () => {
            chosenVersion = version.id;
            renderVersions();
            void loadLoaderBuilds();
            refreshFooter();
          },
        },
        h("span", { class: "id" }, version.id),
        h("span", { class: "when" }, version.releaseTime.slice(0, 10)),
      );
      list.appendChild(row);
    }

    if (shown.length === 0) {
      list.appendChild(h("div", { style: "padding:14px;color:var(--faint);font-size:13px" }, "No versions match."));
    }
  }

  function renderLoader(): void {
    clear(loaderSlot);

    const segmented = h("div", { class: "segmented" });
    for (const choice of ["none", "fabric", "quilt"] as LoaderChoice[]) {
      segmented.appendChild(
        h(
          "button",
          {
            "aria-pressed": loader === choice,
            onclick: () => {
              loader = choice;
              chosenBuild = "";
              renderLoader();
              void loadLoaderBuilds();
            },
          },
          choice === "none" ? "Vanilla" : choice[0]!.toUpperCase() + choice.slice(1),
        ),
      );
    }

    loaderSlot.appendChild(h("label", {}, "Mod loader"));
    loaderSlot.appendChild(segmented);

    if (loader !== "none") {
      const select = h("select", {
        style: "margin-top:10px",
        onchange: (event: Event) => {
          chosenBuild = (event.target as HTMLSelectElement).value;
        },
      }) as HTMLSelectElement;

      select.appendChild(h("option", { value: "" }, "Latest stable build"));
      for (const build of loaderBuilds) {
        select.appendChild(
          h("option", { value: build.version, selected: build.version === chosenBuild }, build.version),
        );
      }
      loaderSlot.appendChild(select);
    }
  }

  async function loadLoaderBuilds(): Promise<void> {
    if (loader === "none" || !chosenVersion) {
      loaderBuilds = [];
      renderLoader();
      return;
    }
    try {
      const reply = await rpc<{ loaders: LoaderBuild[] }>("loader.list", {
        kind: loader,
        version: chosenVersion,
      });
      loaderBuilds = reply.loaders ?? [];
    } catch {
      loaderBuilds = [];
    }
    renderLoader();
  }

  async function create(): Promise<void> {
    busy = true;
    refreshFooter();
    clear(footNote);

    const params: Record<string, unknown> = {
      name: nameInput.value.trim(),
      version: chosenVersion,
    };
    if (loader !== "none") {
      params["loader"] = chosenBuild ? { kind: loader, version: chosenBuild } : { kind: loader };
    }

    try {
      await rpc("instance.create", params);
      scrim.remove();
      onCreated();
    } catch (error) {
      busy = false;
      refreshFooter();
      footNote.appendChild(
        h("span", { style: "color:var(--danger);font-size:12.5px" }, String((error as Error).message)),
      );
    }
  }

  const dialog = h(
    "div",
    { class: "dialog" },
    h("div", { class: "dialog-head" }, h("h2", {}, "New instance")),
    h(
      "div",
      { class: "dialog-body" },
      h("div", {}, h("label", {}, "Name"), nameInput),
      h(
        "div",
        {},
        h(
          "div",
          { class: "row", style: "justify-content:space-between;align-items:flex-end" },
          h("label", { style: "margin:0" }, "Minecraft version"),
          h(
            "label",
            { class: "row", style: "gap:6px;margin:0;cursor:pointer" },
            h("input", {
              type: "checkbox",
              onchange: (event: Event) => {
                showSnapshots = (event.target as HTMLInputElement).checked;
                renderVersions();
              },
            }),
            "Snapshots",
          ),
        ),
        h("div", { style: "height:8px" }),
        search,
        h("div", { style: "height:8px" }),
        h("div", { class: "picker" }, list),
      ),
      loaderSlot,
    ),
    h(
      "div",
      { class: "dialog-foot" },
      footNote,
      h("button", { class: "btn", onclick: () => scrim.remove() }, "Cancel"),
      createButton,
    ),
  );

  scrim.appendChild(dialog);
  scrim.addEventListener("click", (event) => {
    if (event.target === scrim) scrim.remove();
  });
  document.body.appendChild(scrim);
  nameInput.focus();
  refreshFooter();
  renderLoader();

  void (async () => {
    try {
      const reply = await rpc<{ versions: Version[] }>("version.list", { limit: 200 });
      versions = reply.versions ?? [];
    } catch {
      versions = [];
    }
    renderVersions();
  })();
}

export const createIcon = () => svg(icons.plus, 15);
