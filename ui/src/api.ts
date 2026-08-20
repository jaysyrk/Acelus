export type Json = Record<string, unknown>;

export interface DaemonEvent {
  method: string;
  params: Json;
}

export interface RpcFailure {
  code: number;
  message: string;
}

export class RpcError extends Error {
  readonly code: number;

  constructor(failure: RpcFailure) {
    super(failure.message);
    this.name = "RpcError";
    this.code = failure.code;
  }
}

export const codes = {
  authRequired: -32010,
  authExpired: -32011,
  xboxAccountMissing: -32012,
  xboxChildAccount: -32013,
  xboxRegionUnavailable: -32014,
  xboxAccountBanned: -32015,
  xboxAdultVerificationRequired: -32016,
  entitlementMissing: -32017,
  entitlementSignatureInvalid: -32018,
  azureAppUnapproved: -32019,
  notFound: -32001,
  alreadyExists: -32002,
  network: -32003,
  integrityFailed: -32004,
} as const;

type Listener = (event: DaemonEvent) => void;

const listeners = new Set<Listener>();

export function onDaemonEvent(listener: Listener): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function emitDaemonEvent(event: DaemonEvent): void {
  for (const listener of listeners) listener(event);
}

function inTauri(): boolean {
  return "__TAURI_INTERNALS__" in window;
}

interface Backend {
  connect(): Promise<boolean>;
  rpc(method: string, params: Json): Promise<Json>;
}

let backend: Promise<Backend> | null = null;

async function load(): Promise<Backend> {
  if (inTauri()) {
    const { invoke } = await import("@tauri-apps/api/core");
    const { listen } = await import("@tauri-apps/api/event");

    await listen<DaemonEvent>("daemon", (event) => emitDaemonEvent(event.payload));

    return {
      connect: () => invoke<boolean>("connect"),
      rpc: (method, params) => invoke<Json>("rpc", { method, params }),
    };
  }

  const { mockBackend } = await import("./mock");
  return mockBackend();
}

function resolveBackend(): Promise<Backend> {
  if (!backend) backend = load();
  return backend;
}

export async function connect(): Promise<boolean> {
  try {
    return await (await resolveBackend()).connect();
  } catch {
    return false;
  }
}

export async function rpc<T = Json>(method: string, params: Json = {}): Promise<T> {
  try {
    return (await (await resolveBackend()).rpc(method, params)) as T;
  } catch (raw) {
    throw toRpcError(raw);
  }
}

function toRpcError(raw: unknown): RpcError {
  if (raw instanceof RpcError) return raw;
  if (raw && typeof raw === "object" && "message" in raw) {
    const failure = raw as Partial<RpcFailure>;
    return new RpcError({
      code: typeof failure.code === "number" ? failure.code : 0,
      message: String(failure.message ?? "the request failed"),
    });
  }
  return new RpcError({ code: 0, message: String(raw) });
}

export function explain(error: unknown): { title: string; detail: string; link?: string } {
  const failure = toRpcError(error);
  switch (failure.code) {
    case codes.azureAppUnapproved:
      return {
        title: "Mojang has not approved this application yet",
        detail:
          "Sign in works up to the last step. Minecraft's API refuses unapproved applications, so approval has to land before an account can be added.",
        link: "https://aka.ms/mce-reviewappid",
      };
    case codes.xboxAccountMissing:
      return {
        title: "This Microsoft account has no Xbox profile",
        detail: "Sign in once at xbox.com to create one, then try again.",
        link: "https://www.xbox.com",
      };
    case codes.xboxChildAccount:
      return {
        title: "This is a child account",
        detail: "It has to be added to a Microsoft family group before it can sign in.",
      };
    case codes.xboxRegionUnavailable:
      return {
        title: "Xbox Live is not available in this region",
        detail: "The account's country or region cannot use Xbox Live, which Minecraft sign in requires.",
      };
    case codes.xboxAccountBanned:
      return { title: "This Xbox account is banned", detail: "Xbox Live refused the sign in." };
    case codes.xboxAdultVerificationRequired:
      return {
        title: "This account needs adult verification",
        detail: "Complete verification on the Xbox website, then try again.",
      };
    case codes.entitlementMissing:
      return {
        title: "This account does not own Minecraft: Java Edition",
        detail: "Acelus will not launch without a verified entitlement.",
      };
    case codes.entitlementSignatureInvalid:
      return {
        title: "The ownership proof could not be verified",
        detail:
          "The entitlement signature did not match Mojang's key. The response was altered in transit, or Mojang rotated the key.",
      };
    case codes.network:
      return { title: "Could not reach the network", detail: failure.message };
    case codes.integrityFailed:
      return {
        title: "A file did not match its published digest",
        detail: failure.message,
      };
    default:
      return { title: "Something went wrong", detail: failure.message };
  }
}
