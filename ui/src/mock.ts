import { emitDaemonEvent, type Json } from "./api";

const instances: Json[] = [
  {
    id: "modded",
    name: "Modded",
    version: "1.21.11",
    loader: { kind: "fabric", version: "0.19.3" },
    path: "/home/jaysyrk/.local/share/acelus/instances/modded",
    installed: true,
    lastPlayed: 1755640000,
    contentSize: 411041792,
  },
  {
    id: "quilted",
    name: "Quilted",
    version: "1.21.4",
    loader: { kind: "quilt", version: "0.30.0" },
    path: "/home/jaysyrk/.local/share/acelus/instances/quilted",
    installed: true,
    lastPlayed: null,
    contentSize: 411041792,
  },
  {
    id: "survival",
    name: "Survival",
    version: "1.21.11",
    path: "/home/jaysyrk/.local/share/acelus/instances/survival",
    installed: true,
    lastPlayed: 1755500000,
    contentSize: 402653184,
  },
  {
    id: "snapshot-test",
    name: "Snapshot test",
    version: "26.2",
    path: "/home/jaysyrk/.local/share/acelus/instances/snapshot-test",
    installed: false,
    lastPlayed: null,
    contentSize: 0,
  },
];

const versions: Json[] = [
  { id: "26.2", type: "release", releaseTime: "2026-08-04T11:12:00+00:00" },
  { id: "26.2-rc1", type: "snapshot", releaseTime: "2026-07-28T09:40:00+00:00" },
  { id: "1.21.11", type: "release", releaseTime: "2026-02-18T13:05:00+00:00" },
  { id: "1.21.10", type: "release", releaseTime: "2025-12-02T10:22:00+00:00" },
  { id: "1.21.4", type: "release", releaseTime: "2024-12-03T10:12:00+00:00" },
  { id: "1.20.1", type: "release", releaseTime: "2023-06-12T13:25:00+00:00" },
  { id: "1.12.2", type: "release", releaseTime: "2017-09-18T09:39:00+00:00" },
];

const accounts: Json[] = [];

let activeAccount: string | null = null;
let running: Json | null = null;

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function simulateInstall(id: string): Promise<void> {
  const total = 411041792;
  const files = 72;
  const phases: Array<[string, number]> = [
    ["resolve", 0.02],
    ["download", 0.86],
    ["verify", 0.9],
    ["extract", 0.94],
    ["link", 1],
  ];

  for (const [phase, upTo] of phases) {
    const steps = phase === "download" ? 26 : 3;
    for (let step = 1; step <= steps; step += 1) {
      const fraction = (upTo * step) / steps;
      emitDaemonEvent({
        method: "install.progress",
        params: {
          jobId: id,
          phase,
          completedBytes: Math.round(total * fraction),
          totalBytes: total,
          completedFiles: Math.round(files * fraction),
          totalFiles: files,
          reusedBytes: Math.round(total * fraction * 0.62),
          current: phase === "download" ? "libraries/net/fabricmc/fabric-loader.jar" : null,
        },
      });
      await delay(55);
    }
  }

  const instance = instances.find((entry) => entry["id"] === id);
  if (instance) instance["installed"] = true;
  emitDaemonEvent({ method: "install.progress", params: { jobId: id, phase: "done" } });
}

async function simulateLogs(sessionId: string): Promise<void> {
  const lines = [
    "[main/INFO]: Loading Minecraft 1.21.11 with Fabric Loader 0.19.3",
    "[main/INFO]: Loading 4 mods:",
    "[main/INFO]:  - fabric-api 0.115.0",
    "[main/INFO]:  - sodium 0.6.13",
    "[Render thread/INFO]: Setting user: jaysyrk",
    "[Render thread/INFO]: Backend library: LWJGL version 3.3.3",
    "[Render thread/INFO]: Reloading ResourceManager: vanilla, fabric",
    "[Worker-Main-1/INFO]: Found unifont_all_no_pua-15.1.05.hex, loading",
    "[Render thread/INFO]: OpenAL initialized on device Family 17h/19h HD Audio",
    "[Render thread/INFO]: Sound engine started",
    "[Render thread/INFO]: Created: 1024x512x4 minecraft:textures/atlas/blocks.png",
    "[Render thread/INFO]: Reached the main menu",
  ];

  for (const line of lines) {
    await delay(230);
    emitDaemonEvent({ method: "log.line", params: { sessionId, line, stream: "stdout" } });
  }
}

export function mockBackend() {
  return {
    async connect() {
      return true;
    },

    async rpc(method: string, params: Json): Promise<Json> {
      await delay(40);

      switch (method) {
        case "daemon.info":
          return { version: "0.1.0", rpcVersion: 1, dataDir: "/home/jaysyrk/.local/share/acelus" };

        case "instance.list":
          return { instances };

        case "import.scan":
          return {
            found: [
              {
                path: "/home/jaysyrk/.var/app/org.prismlauncher.PrismLauncher/data/PrismLauncher/instances/main",
                name: "main",
                version: "1.21.11",
                loader: { kind: "fabric", version: "0.18.4" },
                memoryMegabytes: 6144,
              },
              {
                path: "/home/jaysyrk/.local/share/PrismLauncher/instances/atm10",
                name: "All the Mods 10",
                version: "1.21.1",
                loader: null,
                blockedBy: "NeoForge",
              },
            ],
          };

        case "import.run":
          return { instance: { id: "main", path: "/home/jaysyrk/.local/share/acelus/instances/main" }, copiedBytes: 2_791_728_742 };

        case "instance.create": {
          const name = String(params["name"] ?? "New instance");
          const id = name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
          const created: Json = {
            id,
            name,
            version: String(params["version"] ?? "1.21.11"),
            loader: params["loader"] ?? undefined,
            path: `/home/jaysyrk/.local/share/acelus/instances/${id}`,
            installed: false,
            lastPlayed: null,
            contentSize: 0,
          };
          instances.unshift(created);
          return { instance: created };
        }

        case "instance.delete": {
          const index = instances.findIndex((entry) => entry["id"] === params["id"]);
          if (index >= 0) instances.splice(index, 1);
          return {};
        }

        case "version.list":
          return { versions, latest: { release: "26.2", snapshot: "26.2-rc1" } };

        case "loader.list":
          return {
            loaders:
              params["kind"] === "quilt"
                ? [{ version: "0.30.0", stable: true }, { version: "0.29.2", stable: true }]
                : [{ version: "0.19.3", stable: true }, { version: "0.19.2", stable: true }],
          };

        case "install.run":
          void simulateInstall(String(params["id"]));
          return { jobId: String(params["id"]) };

        case "account.list":
          return { accounts, active: activeAccount };

        case "account.beginLogin":
          setTimeout(() => {
            emitDaemonEvent({
              method: "account.loginComplete",
              params: {
                jobId: "login",
                error: {
                  code: -32019,
                  message:
                    "the Minecraft services API refused this application, which means Mojang has not approved it yet; request approval at https://aka.ms/mce-reviewappid",
                },
              },
            });
          }, 2600);
          return { jobId: "login", userCode: "NW7ZB8E5", verificationUri: "https://www.microsoft.com/link" };

        case "account.select":
          activeAccount = String(params["uuid"]);
          return {};

        case "launch.run": {
          const sessionId = "s-4f21c9";
          running = { sessionId, instanceId: String(params["id"]), pid: 48213, startedAt: Date.now() };
          void simulateLogs(sessionId);
          return running;
        }

        case "session.list":
          return { sessions: running ? [running] : [] };

        case "session.stop":
          running = null;
          return {};

        case "log.tail":
          return { lines: [] };

        case "verify.run":
          return { ok: true, problems: [] };

        default:
          return {};
      }
    },
  };
}
