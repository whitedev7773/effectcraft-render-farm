export type Theme = "dark" | "light" | "system";
export type Distribution = "auto" | "frames" | "percent";
export type Page = "queue" | "queue_tools" | "monitor" | "nodes" | "logs" | "settings";
export type WindowOperation = "welcome" | "workspace" | "drag" | "minimize" | "maximize" | "close"
  | "north" | "south" | "east" | "west" | "north_east" | "north_west" | "south_east" | "south_west";
export type Status = "Pending" | "Preparing" | "Waiting" | "Rendering" | "Encoding" | "Completed" | "Failed" | "Interrupted";
export type Preview = { frame: number; url: string };
export type Composition = {
  name: string; frames: number; width: number; height: number;
  rate: { num: number; den: number }; work_start: number; work_end: number;
};
export type RenderOptions = {
  automatic: boolean; nvenc: boolean; ffmpeg: string; adaptive: boolean; target_seconds: number;
  retries: number; previews: boolean; preview_seconds: number; poll_ms: number; draft: boolean;
  video_bitrate: number; audio: boolean; durable_writes: boolean;
};
export type RenderDefaults = {
  render: RenderOptions; distribution: Distribution; chunk_frames: number; chunk_percent: number;
  max_workers: number; gpu: boolean;
};
export type TaskOptions = RenderDefaults & {
  final_video: string; range_start: number; range_end: number;
};
export type Task = TaskOptions & {
  id: number; project: string; composition: Composition; status: Status; status_label: string;
  done: number; detail: string; preview: Preview | null;
};
export type Worker = {
  id: number; ip: string; label: string; enabled: boolean; connected: boolean; ready: boolean;
  range: [number, number] | null; done: number; completed: number; fps: number; bytes: number;
  seconds: number; elapsed: number | null; profile: string; preview: Preview | null;
};
export type ImportDraft = {
  project: string; compositions: Composition[]; selected: number | null; options: TaskOptions;
  loading: boolean; preview_busy: boolean; preview: Preview | null; error: string;
};
export type Appearance = { theme: Theme; reduced_motion: boolean; check_updates: boolean };
export type Connection = { host: boolean; address: string; token: string; local_worker: boolean };
export type Settings = Appearance & RenderDefaults & {
  host: boolean; host_address: string; client_address: string; token: string; local_worker: boolean;
  queue_cards: boolean; cli: string;
};
export type Snapshot = {
  version: string; windows: boolean; settings: Settings; tasks: Task[]; import: ImportDraft | null;
  workers: Worker[]; hosts: { name: string; address: string }[];
  connected: boolean; preparing: boolean; stopping: boolean; locked: boolean; dispatch: boolean;
  automatic: boolean; active_task: number | null; blocker: string | null; error: string;
  discovering: boolean; discovery_status: string;
  render_state: {
    done: number; total: number; failures: number; encoding: boolean; status: string; elapsed: number | null;
    fps: number; comp: string | null; local_range: [number, number, number] | null; profile: string;
    output: string; frames: string; messages: string[]; timeline: number[]; preview: Preview | null;
  };
  inspect: { start: number; end: number; busy: boolean; status: string; preview: Preview | null; can_load: boolean; can_render: boolean };
  engine: {
    local: string | null; latest: string | null; target: string | null; peer: string | null; prompt: boolean;
    checking: boolean; installing: boolean; queued: boolean; status: string; missing: string; bytes: number;
  };
  update: { target: string | null; body: string | null; prompt: boolean; status: string; busy: boolean; installing: boolean; bytes: number };
};

export type Command =
  | { action: "snapshot"; logs: boolean }
  | { action: "dismiss_error" | "open_project" | "close_import" | "pick_ffmpeg" | "discover" |
      "connect" | "disconnect" | "close_when_idle" | "render" | "check_engine" | "check_app" | "cancel_engine_update" }
  | { action: "select_composition"; index: number }
  | { action: "configure_import"; options: TaskOptions; commit: boolean; preview: boolean }
  | { action: "pick_output"; suggested: string }
  | { action: "save_task"; id: number; options: TaskOptions; retry: boolean }
  | { action: "remove_task"; id: number }
  | { action: "remove_tasks"; ids: number[] }
  | { action: "move_task"; id: number; direction: -1 | 1 }
  | { action: "queue_view"; cards: boolean }
  | ({ action: "set_connection" } & Connection)
  | ({ action: "set_appearance" } & Appearance)
  | ({ action: "set_render" } & RenderDefaults)
  | { action: "worker_participation"; id: number; enabled: boolean }
  | { action: "inspect"; frame: number; render: boolean }
  | { action: "update_prompt"; engine: boolean; open: boolean }
  | { action: "install_update"; engine: boolean }
  | { action: "window"; operation: WindowOperation }
  | { action: "open_location"; location: "engine_source" | "engine_release" | "farm_release" | "log_directory" | "output_directory" | "project_directory" | "frame_directory"; task?: number };

export type Action = <T = null>(command: Command, success?: string) => Promise<T | undefined>;
declare global { interface Window { ipc?: { postMessage(message: string): void } } }

let sequence = 0;
type Reply = { id: number; value?: unknown; error?: string };
const awaiting = new Map<number, (reply: Reply) => void>();
window.addEventListener("farm:reply", (event) => {
  const reply = (event as CustomEvent<Reply>).detail;
  awaiting.get(reply.id)?.(reply);
});

export function request<T>(command: Command): Promise<T> {
  return new Promise((resolve, reject) => {
    if (!window.ipc) {
      reject(new Error("Render Farm 데스크톱 앱에서 이 화면을 열어 주세요."));
      return;
    }
    const id = ++sequence;
    const picker = ["open_project", "pick_output", "pick_ffmpeg"].includes(command.action);
    const timer = setTimeout(() => {
      awaiting.delete(id);
      reject(new Error("앱의 응답을 받지 못했습니다. 잠시 후 다시 시도하세요."));
    }, picker ? 600_000 : 12_000);
    awaiting.set(id, ({ value, error }) => {
      clearTimeout(timer);
      awaiting.delete(id);
      if (error) reject(new Error(error));
      else resolve(value as T);
    });
    window.ipc.postMessage(JSON.stringify({ id, ...command }));
  });
}

export const isActive = (status: Status) => ["Preparing", "Waiting", "Rendering", "Encoding"].includes(status);
export const filename = (path: string) => path.split(/[\\/]/).filter(Boolean).at(-1) ?? "";
export const count = (value: number) => new Intl.NumberFormat("ko-KR").format(value);
export const elapsed = (seconds: number | null) => {
  if (seconds === null || !Number.isFinite(seconds) || seconds < 0) return "—";
  const value = Math.floor(seconds);
  const units = [Math.floor(value / 3600), Math.floor((value % 3600) / 60), value % 60];
  return units.map((part) => String(part).padStart(2, "0")).join(":");
};

export const percent = (done: number, total: number) => total > 0 ? Math.min(100, Math.max(0, 100 * done / total)) : 0;
export const sizeMiB = (bytes: number) => (bytes / 1048576).toFixed(1);
export const message = (error: unknown) => error instanceof Error ? error.message : String(error);
