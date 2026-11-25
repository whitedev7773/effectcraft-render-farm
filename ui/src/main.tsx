import { Button, Spinner, Toast, toast } from "@heroui/react";
import { CircleHelp, Play, Server, Square, X } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { createRoot } from "react-dom/client";
import { message, request, type Action, type Command, type Page, type Snapshot, type Theme, type WindowOperation } from "./api";
import { Dialog, IconButton, Notice } from "./components";
import { InspectDialog, LogsPage, MonitorPage, WorkerDialog } from "./operations";
import { ServerQueue } from "./server-queue";
import { NodesPage, SettingsPage, UpdateDialog, type SettingsTab } from "./settings";
import { ImportDialog, QueuePage, TaskDialog } from "./tasks";
import { Welcome, WorkspaceShell } from "./workspace";
import { transitionView, type ChangeTheme } from "./motion";
import { applyWindowGeometry, scheduleWindowCornerColor } from "./window-geometry";
import brand from "../../assets/app-icon/farm-256.png";
import "./styles.css";
import "./reference.css";
import "./motion.css";

function App() {
  const [welcome, setWelcome] = useState(true);
  const [maximized, setMaximized] = useState(false);
  const [page, setPage] = useState<Page>("queue");
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("appearance");
  const [data, setData] = useState<Snapshot>();
  const [loadError, setLoadError] = useState("");
  const [taskId, setTaskId] = useState<number>();
  const [workerId, setWorkerId] = useState<number>();
  const [inspect, setInspect] = useState(false);
  const [update, setUpdate] = useState<"engine" | "farm">();
  const [closeRequested, setCloseRequested] = useState(false);
  const [closing, setClosing] = useState(false);
  const [systemDark, setSystemDark] = useState(() => window.matchMedia("(prefers-color-scheme: dark)").matches);
  const pageRef = useRef<Page>(page);
  const serial = useRef(0);
  const picker = useRef(false);
  const choosingRole = useRef(false);
  const focus = useRef<HTMLElement>(null);
  const themeAnchor = useRef<{ theme: Theme; element: HTMLElement | null }>(null);
  const theme = data?.settings.theme;
  const reducedMotion = Boolean(data?.settings.reduced_motion);
  const dark = theme === "system" ? systemDark : theme === "dark";

  const refresh = useCallback(async () => {
    if (picker.current) return;
    const version = ++serial.current;
    const snapshot = await request<Snapshot>({ action: "snapshot", logs: pageRef.current === "logs" });
    if (version !== serial.current) return;
    setData(snapshot);
    setLoadError("");
  }, []);

  const navigate = (next: Page) => {
    const shouldAnimate = !welcome && pageRef.current !== next && focus.current;
    pageRef.current = next;
    const commit = () => {
      if (pageRef.current !== next) return;
      flushSync(() => setPage(next));
      focus.current?.focus({ preventScroll: true });
    };
    if (shouldAnimate) transitionView(commit);
    else commit();
    void refresh().catch(() => undefined);
  };

  const control = useCallback((operation: WindowOperation) => {
    void request<null>({ action: "window", operation }).catch((error) => toast.danger(message(error)));
  }, []);

  useEffect(() => {
    let stopped = false;
    let timer = 0;
    async function tick() {
      try {
        await refresh();
      } catch (error) {
        if (!stopped) setLoadError(message(error));
      }
      if (!stopped && window.ipc) timer = window.setTimeout(() => void tick(), 600);
    }
    void tick();
    return () => { stopped = true; window.clearTimeout(timer); };
  }, [refresh]);

  useEffect(() => {
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const change = () => setSystemDark(query.matches);
    const close = () => setCloseRequested(true);
    const windowMode = (event: Event) => setMaximized(Boolean((event as CustomEvent<boolean>).detail));
    query.addEventListener("change", change);
    window.addEventListener("farm:close-request", close);
    window.addEventListener("farm:window-mode", windowMode);
    return () => {
      query.removeEventListener("change", change);
      window.removeEventListener("farm:close-request", close);
      window.removeEventListener("farm:window-mode", windowMode);
    };
  }, []);

  useLayoutEffect(() => {
    const root = document.documentElement;
    root.toggleAttribute("data-reduce-motion", reducedMotion);
    if (!theme) return;

    const requested = themeAnchor.current?.theme === theme ? themeAnchor.current : null;
    if (requested) themeAnchor.current = null;
    const next = dark ? "dark" : "light";
    if (root.dataset.theme === next) return;
    const anchor = requested?.element?.isConnected ? requested.element : document.querySelector<HTMLElement>(".theme-switch");
    let current = true;
    const apply = () => {
      if (!current) return;
      root.classList.toggle("dark", dark);
      root.classList.toggle("light", !dark);
      root.dataset.theme = next;
      scheduleWindowCornerColor();
    };
    if (!root.dataset.theme || !anchor) {
      apply();
      return;
    }
    const transition = transitionView(apply, anchor);
    return () => { current = false; transition?.skipTransition(); };
  }, [dark, reducedMotion, theme]);

  const act: Action = async <T,>(command: Command, success?: string) => {
    const choosing = ["open_project", "pick_ffmpeg", "pick_output"].includes(command.action);
    if (picker.current) return undefined;
    if (choosing) picker.current = true;
    try {
      const response = await request<T>(command);
      if (success) toast.success(success);
      return response;
    } catch (error) {
      toast.danger(message(error));
      return undefined;
    } finally {
      if (choosing) picker.current = false;
      if (!["pick_ffmpeg", "pick_output"].includes(command.action)) {
        await refresh().catch((error) => setLoadError(message(error)));
      }
    }
  };

  if (!data) return <main className="startup">
    <img src={brand} alt="" width="52" height="52" />
    <h1>EffectCraft Render Farm</h1>
    {loadError ? <Notice title="작업 공간을 불러오지 못했습니다" danger>{loadError}</Notice> : <Spinner aria-label="작업 공간을 불러오는 중" />}
    {loadError && window.ipc && <Button variant="secondary" onPress={() => void refresh().catch((error) => setLoadError(message(error)))}>다시 시도</Button>}
    <Button variant="ghost" size="sm" onPress={() => control("close")}>닫기</Button>
  </main>;

  const task = data.tasks.find((item) => item.id === taskId);
  const worker = data.workers.find((item) => item.id === workerId);
  const updateOpen = update ?? (data.engine.peer && data.engine.prompt && !data.import && !task && !worker && !inspect && !closeRequested ? "engine" : undefined);
  const showUpdate = (engine: boolean) => setUpdate(engine ? "engine" : "farm");
  const showFarmUpdate = () => showUpdate(false);
  const closeUpdate = () => {
    if (updateOpen) void act({ action: "update_prompt", engine: updateOpen === "engine", open: false });
    setUpdate(undefined);
  };

  const changeTheme: ChangeTheme = (next, element) => {
    if (next === theme) return;
    const pending = { theme: next, element };
    themeAnchor.current = pending;
    void act({ action: "set_appearance", theme: next, reduced_motion: reducedMotion, check_updates: data.settings.check_updates })
      .then((saved) => { if (saved === undefined && themeAnchor.current === pending) themeAnchor.current = null; });
  };
  const enterWorkspace = (start: Page) => {
    setWelcome(false);
    control("workspace");
    navigate(start);
  };
  async function chooseRole(host: boolean) {
    if (!data || choosingRole.current) return;
    if (host !== data.settings.host) {
      choosingRole.current = true;
      const { settings } = data;
      const saved = await act({ action: "set_connection", host, address: host ? settings.host_address : settings.client_address,
        token: settings.token, local_worker: settings.local_worker });
      choosingRole.current = false;
      if (saved === undefined) return;
    }
    enterWorkspace(host ? "queue" : "nodes");
  }
  const homeAction = () => {
    if (data.locked || data.import) return;
    setWelcome(true);
    control("welcome");
  };
  const notices = <>
    {loadError && <Notice title="연결 상태를 새로 고치지 못했습니다" danger>{loadError}</Notice>}
    {data.error && <div className="dismissible-alert"><Notice title="작업을 확인해 주세요" danger>{data.error}</Notice>
      <IconButton label="오류 알림 닫기" icon={X} onPress={() => void act({ action: "dismiss_error" })} /></div>}
    {data.engine.missing && page !== "settings" && <div className="install-notice">
      <Notice title="EffectCraft 설치가 필요합니다">시험 렌더와 작업 PC 연결에는 EffectCraft와 CLI가 필요합니다.</Notice>
      <Button size="sm" variant="outline" onPress={() => void act({ action: "open_location", location: "engine_release" })}>공식 설치 페이지</Button>
      <Button size="sm" variant="ghost" onPress={() => void act({ action: "check_engine" })}>다시 확인</Button>
    </div>}
  </>;

  return <>
    {welcome ? <Welcome data={data} act={act} chooseRole={(host) => void chooseRole(host)} showUpdate={showFarmUpdate}
      changeTheme={changeTheme} control={control} maximized={maximized} /> :
      <WorkspaceShell data={data} page={page} navigate={navigate} homeAction={homeAction}
        showFarmUpdate={showFarmUpdate} changeTheme={changeTheme} control={control} maximized={maximized} focus={focus}>
        {page === "queue" && data.settings.host ? <div className="reference-queue-view">
          <div className="reference-notices">{notices}</div>
          <ServerQueue data={data} act={act} navigate={navigate} openTask={setTaskId} openWorker={setWorkerId} />
        </div> : <div className="workspace-page" key={page}>
          <div className="workspace-page-scroll">
            {notices}
            {page === "queue_tools" && data.settings.host && <QueuePage data={data} act={act} openTask={setTaskId} />}
            {page === "monitor" && <MonitorPage data={data} act={act} navigate={navigate} openWorker={setWorkerId} inspect={() => setInspect(true)} />}
            {page === "nodes" && <NodesPage data={data} act={act} openWorker={setWorkerId} />}
            {page === "logs" && <LogsPage data={data} act={act} />}
            {page === "settings" && <SettingsPage data={data} act={act} showUpdate={showUpdate}
              navigate={navigate} tab={settingsTab} changeTab={setSettingsTab} changeTheme={changeTheme} />}
          </div>
          {(!data.settings.host || data.locked) && <SessionBar data={data} navigate={navigate} act={act} closing={closing} />}
        </div>}
      </WorkspaceShell>}

    <Toast.Provider placement="bottom end" className="farm-toasts" width={360} maxVisibleToasts={3} />
    {data.import && (!updateOpen || !update) && <ImportDialog key={data.import.project} draft={data.import} act={act}
      nativeBusy={!data.engine.local || data.engine.installing || data.engine.queued} done={() => welcome ? enterWorkspace("queue") : navigate("queue")} />}
    {task && !data.import && !updateOpen && <TaskDialog key={task.id} task={task} act={act} close={() => setTaskId(undefined)} active={data.active_task === task.id} />}
    {worker && !data.import && !updateOpen && <WorkerDialog worker={worker} data={data} act={act} close={() => setWorkerId(undefined)} />}
    {inspect && !data.import && !updateOpen && <InspectDialog key={`${data.inspect.start}-${data.inspect.end}`} data={data} act={act} close={() => setInspect(false)} />}
    {updateOpen && !closeRequested && <UpdateDialog data={data} act={act} engine={updateOpen === "engine"} close={closeUpdate} />}
    {closeRequested && <Dialog title="Render Farm을 종료할까요?" onClose={() => setCloseRequested(false)}
      footer={(dismiss) => <><Button variant="secondary" onPress={dismiss}>계속 사용</Button>
        <Button variant="danger" onPress={() => { setClosing(true); setCloseRequested(false); void act({ action: "close_when_idle" }); }}>작업 정리 후 종료</Button></>}>
      <p>연결을 닫고 진행 중인 구간, 출력 또는 설치가 정리되면 앱을 종료합니다.</p>
    </Dialog>}
  </>;
}

function SessionBar({ data, act, navigate, closing }: { data: Snapshot; act: Action; navigate: (page: Page) => void; closing: boolean }) {
  const selected = data.workers.filter((worker) => worker.connected && worker.enabled).length;
  const busy = data.preparing || data.stopping || closing;
  const waiting = data.connected && data.settings.host && !data.dispatch && !busy;
  const hint = closing ? "진행 중인 작업을 정리한 뒤 앱을 종료합니다."
    : data.stopping ? "진행 중인 구간과 출력을 마치는 중입니다."
    : waiting ? (selected ? `선택된 작업 PC ${selected}대 · 렌더를 시작할 수 있습니다.` : "렌더에 참여할 작업 PC의 연결을 기다리는 중입니다.")
    : data.locked ? data.render_state.status || "연결 상태를 확인하고 있습니다."
    : data.blocker || "저장한 호스트로 연결할 수 있습니다.";
  const connect = async () => {
    if (data.blocker) navigate(!data.engine.local ? "settings" : "nodes");
    else if (await act({ action: "connect" }) !== undefined) navigate("monitor");
  };

  return <footer className="session-bar">
    <div className="session-hint" title={hint}>
      {busy ? <Spinner size="sm" /> : data.connected ? <span className="connection-dot online" /> : <CircleHelp size={17} />}
      <span>{hint}</span>
    </div>
    <div className="session-actions">
      {waiting && <Button variant="primary" isDisabled={!selected || Boolean(data.engine.target)} onPress={() => void act({ action: "render" })}>
        <Play size={16} /> {data.automatic ? "연결된 PC로 계속" : "렌더 시작"}</Button>}
      {data.locked ? <Button variant="outline" isDisabled={data.stopping || closing}
        onPress={() => void act({ action: "disconnect" })}><Square size={14} /> {data.preparing ? "준비 취소" : "연결 종료"}</Button>
        : <Button variant="primary" isDisabled={closing || data.engine.checking || data.update.installing || data.engine.installing}
          onPress={() => void connect()}><Server size={17} /> {data.blocker ? (!data.engine.local ? "설치 상태 확인" : "연결 설정") : "호스트에 연결"}</Button>}
    </div>
  </footer>;
}

const root = document.getElementById("root");
applyWindowGeometry();
if (root) createRoot(root).render(<App />);
