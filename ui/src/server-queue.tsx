import { Button, Card, Checkbox, ScrollShadow, Table, toast } from "@heroui/react";
import { ArrowDown, ArrowUp, FolderOpen, Settings2, Square, Trash2 } from "lucide-react";
import { useRef, useState, type CSSProperties } from "react";
import { count, filename, isActive, percent, type Action, type Page, type Snapshot, type Task } from "./api";
import { Dialog, Empty, FrameImage, Graphic, IconButton, PageHeading, Progress } from "./components";
import pcEmpty from "../../assets/ui/figma/pc-link-slash.svg";
import pcDisplay from "../../assets/ui/figma/pc-display.svg";
import queueEmpty from "../../assets/ui/figma/queue-empty.svg";
import renderEmpty from "../../assets/ui/figma/status-empty-broadcast.svg";
import check from "../../assets/ui/figma/selection-check.svg";
import external from "../../assets/ui/figma/queue-external-link.svg";
import add from "../../assets/ui/figma/queue-add.svg";
import play from "../../assets/ui/figma/queue-play.svg";
import horizontal from "../../assets/ui/figma/resize-horizontal.svg";
import vertical from "../../assets/ui/figma/resize-vertical.svg";

type QueueProps = { data: Snapshot; act: Action; navigate: (page: Page) => void; openTask: (id: number) => void; openWorker: (id: number) => void };

export function ServerQueue({ data, act, navigate, openTask, openWorker }: QueueProps) {
  const [selection, setSelection] = useState(new Set<number>());
  const [confirm, setConfirm] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [fade, setFade] = useState(false);
  const [summaryHeight, setSummaryHeight] = useState(260);
  const [pcWidth, setPcWidth] = useState(300);
  const tasks = data.tasks;
  const pending = tasks.filter((task) => task.status === "Pending");
  const selected = tasks.filter((task) => selection.has(task.id));
  const removable = selected.length > 0 && selected.every((task) => !isActive(task.status) && data.active_task !== task.id);
  const participants = data.workers.filter((worker) => worker.enabled && worker.connected);
  const dispatching = data.connected && (data.dispatch || data.render_state.encoding);
  const waitingForNodes = data.connected && !dispatching;
  const cancel = data.preparing || dispatching;
  const startLabel = data.stopping ? "연결 종료 중" : data.preparing ? "준비 취소" : dispatching ? "연결 종료"
    : data.connected && data.automatic ? "연결된 PC로 계속" : "렌더링 시작";

  function start() {
    if (cancel) {
      void act({ action: "disconnect" });
    } else if (waitingForNodes) {
      if (data.engine.target) toast.info("EffectCraft 버전을 맞춘 뒤 렌더링하세요.");
      else if (!participants.length) navigate("nodes");
      else void act({ action: "render" });
    } else if (data.blocker) {
      if (data.blocker.includes("접속 암호")) navigate("nodes");
      else if (!data.engine.local && !data.engine.checking) navigate("settings");
      else toast.info(data.blocker);
    } else {
      void act({ action: "connect" });
    }
  }

  async function remove() {
    setRemoving(true);
    const result = await act({ action: "remove_tasks", ids: selected.map((task) => task.id) }, "선택한 작업을 제거했습니다.");
    setRemoving(false);
    if (result !== undefined) {
      setSelection(new Set());
      setConfirm(false);
    }
  }

  const layout = { "--summary-height": `${summaryHeight}px`, "--pc-width": `${pcWidth}px` } as CSSProperties;
  return <section className="reference-queue" style={layout} aria-label="렌더링 목록">
    <PageHeading className="reference-queue-heading" title="렌더링 목록" description={`현재 ${count(pending.length)}개의 컴포지션이 대기중입니다.`}
      actionsClassName="reference-queue-actions" actions={<>
        {selected.length > 0 && <Button variant="ghost" size="sm" isDisabled={!removable} onPress={() => setConfirm(true)}>
          <Trash2 size={15} /> {selected.length}개 제거
        </Button>}
        <Button variant="secondary" isDisabled={data.locked} onPress={() => void act({ action: "open_project" })}>
          <Graphic src={add} /> 컴포지션 추가
        </Button>
        <Button variant="primary" className="reference-render-action" isDisabled={data.stopping} onPress={start}>
          {cancel ? <Square size={15} /> : <Graphic src={play} />} {startLabel}
        </Button>
      </>} />

    <Table className="reference-queue-table" data-empty={!tasks.length || undefined} data-fade={fade || undefined}>
      <ScrollShadow hideScrollBar size={0} className="reference-queue-scroll" onVisibilityChange={(visibility) => setFade(visibility === "bottom" || visibility === "both")}>
        <Table.Content aria-label="컴포지션 대기열" className="reference-table-content"
          selectionMode="multiple" selectionBehavior="toggle" selectedKeys={selection}
          onSelectionChange={(keys) => setSelection(keys === "all" ? new Set(tasks.map((task) => task.id)) : new Set([...keys].map(Number)))}>
          <Table.Header>
            <Table.Column className="queue-selection-col" aria-label="선택">
              <div className="queue-selection"><span className="queue-grip-space" />
                <SelectionBox name="모든 작업 선택" />
              </div>
            </Table.Column>
            <Table.Column isRowHeader className="queue-comp-col">컴포지션</Table.Column>
            <Table.Column className="queue-project-col">프로젝트 경로</Table.Column>
            <Table.Column className="queue-output-col">출력물 저장 위치</Table.Column>
            <Table.Column className="queue-status-col">상태</Table.Column>
          </Table.Header>
          <Table.Body items={tasks} renderEmptyState={() => <QueueEmptyState />}>
            {(task) => <Table.Row id={task.id}>
              <Table.Cell className="queue-selection-col">
                <div className="queue-selection">
                  <QueueGrip task={task} act={act} />
                  <SelectionBox name={`${task.composition.name} 선택`} />
                </div>
              </Table.Cell>
              <Table.Cell className="queue-comp-col">
                <Button variant="ghost" className="queue-composition" onPress={() => openTask(task.id)}>
                  <span>{task.composition.name}</span><small>{filename(task.project)}</small>
                </Button>
              </Table.Cell>
              <Table.Cell className="queue-project-col"><FolderLink type="project_directory" path={task.project} id={task.id} act={act} /></Table.Cell>
              <Table.Cell className="queue-output-col"><FolderLink type="output_directory" path={task.final_video} id={task.id} act={act} /></Table.Cell>
              <Table.Cell className="queue-status-col">
                <div className="queue-row-status">
                  <TaskStatus task={task} />
                  <div className="queue-inline-actions">
                    {task.status === "Pending" && <>
                      <IconButton label="순서 올리기" icon={ArrowUp} isDisabled={pending[0]?.id === task.id} onPress={() => void act({ action: "move_task", id: task.id, direction: -1 })} />
                      <IconButton label="순서 내리기" icon={ArrowDown} isDisabled={pending.at(-1)?.id === task.id} onPress={() => void act({ action: "move_task", id: task.id, direction: 1 })} />
                    </>}
                    <IconButton label={`${task.composition.name} 작업 상세`} icon={Settings2} onPress={() => openTask(task.id)} />
                  </div>
                </div>
              </Table.Cell>
            </Table.Row>}
          </Table.Body>
        </Table.Content>
      </ScrollShadow>
      {fade && <div className="queue-list-fade" aria-hidden="true" />}
    </Table>

    <SplitHandle axis="height" value={summaryHeight} update={setSummaryHeight} />
    <div className="reference-summary">
      <Card className="reference-summary-card reference-render-summary">
        {data.locked || data.render_state.total > 0 ? <RenderSummary data={data} navigate={navigate} act={act} /> :
          <Empty icon={renderEmpty} title="렌더중인 컴포지션 없음">렌더링이 시작되면 여기에 정보가 표시됩니다.</Empty>}
      </Card>
      <SplitHandle axis="width" value={pcWidth} update={setPcWidth} />
      <Card className="reference-summary-card reference-workers-summary">
        {data.workers.length ? <div className="reference-worker-list">
          <div className="reference-worker-header"><strong>연결된 PC {data.workers.filter((worker) => worker.connected).length}대</strong>
            <Button size="sm" variant="ghost" onPress={() => navigate("nodes")}>관리</Button></div>
          {data.workers.map((worker) => <Button key={worker.id} variant="ghost" className="reference-worker-row" onPress={() => openWorker(worker.id)}>
            <Graphic src={pcDisplay} /><span><strong>{worker.label}</strong><small>{worker.ip}</small></span>
            <small>{worker.connected ? (worker.enabled ? "참여" : "대기") : "연결 종료"}</small>
          </Button>)}
        </div> : <Empty icon={pcEmpty} title="연결된 PC 없음" muted />}
      </Card>
    </div>
    {confirm && <Dialog title="선택한 작업을 제거할까요?" locked={removing} onClose={() => setConfirm(false)}
      footer={(dismiss) => <><Button variant="secondary" onPress={dismiss} isDisabled={removing}>취소</Button>
        <Button variant="danger" onPress={() => void remove()} isDisabled={!removable || removing}>{selected.length}개 제거</Button></>}>
      <p>대기열에서 작업을 제거합니다. 저장된 프레임과 MP4 파일은 보관됩니다.</p>
    </Dialog>}
  </section>;
}

function SelectionBox({ name }: { name: string }) {
  return <Checkbox slot="selection" aria-label={name} className="reference-check">
    <Checkbox.Content>
      <Checkbox.Control><Checkbox.Indicator><Graphic src={check} /></Checkbox.Indicator></Checkbox.Control>
    </Checkbox.Content>
  </Checkbox>;
}

function QueueGrip({ task, act }: { task: Task; act: Action }) {
  const origin = useRef<number | undefined>(undefined);
  const move = (direction: -1 | 1) => void act({ action: "move_task", id: task.id, direction });
  return <Button variant="ghost" isIconOnly aria-label={`${task.composition.name} 순서 변경`} isDisabled={task.status !== "Pending"}
    className="queue-grip" onPointerDown={(event) => {
      if (event.button !== 0 || task.status !== "Pending") return;
      origin.current = event.clientY;
      event.currentTarget.setPointerCapture(event.pointerId);
    }} onPointerUp={(event) => {
      const initial = origin.current;
      origin.current = undefined;
      if (initial === undefined) return;
      const delta = event.clientY - initial;
      if (Math.abs(delta) >= 20) move(delta < 0 ? -1 : 1);
    }} onPointerCancel={() => { origin.current = undefined; }} onKeyDown={(event) => {
      if (event.key === "ArrowUp" || event.key === "ArrowDown") {
        event.preventDefault();
        event.stopPropagation();
        move(event.key === "ArrowUp" ? -1 : 1);
      }
    }}><span aria-hidden="true" /></Button>;
}

function FolderLink({ type, path, id, act }: { type: "project_directory" | "output_directory"; path: string; id: number; act: Action }) {
  const folderPath = path.replace(/[\\/][^\\/]+$/, "");
  const label = type === "project_directory" && folderPath ? folderPath : path;
  return <Button variant="ghost" className="queue-path-link"
    aria-label={`${type === "project_directory" ? "프로젝트 폴더" : "출력 폴더"} 열기: ${path}`}
    onPress={() => void act({ action: "open_location", location: type, task: id })}>
    <span>{label}</span><Graphic src={external} />
  </Button>;
}

function TaskStatus({ task }: { task: Task }) {
  if (!isActive(task.status)) return <span className="reference-task-status" data-state={task.status}>
    {task.status === "Pending" ? "대기중" : task.status_label}
  </span>;
  const progress = percent(task.done, (task.range_end || task.composition.frames) - task.range_start);
  return <div className="reference-task-progress" title={task.status_label}>
    <Progress label={`${task.composition.name} 진행률`} value={progress} /><span>{Math.round(progress)}%</span>
  </div>;
}

function QueueEmptyState() {
  return <Empty icon={queueEmpty} title="대기중인 컴포지션 없음">우측 버튼으로 렌더링할 컴포지션을 추가합니다.</Empty>;
}

function RenderSummary({ data, navigate, act }: Omit<QueueProps, "openTask" | "openWorker">) {
  const render = data.render_state;
  return <div className="reference-active-summary">
    {render.preview && <FrameImage preview={render.preview} />}
    <div className="reference-active-data">
      <strong>{render.comp ?? "렌더 준비 중"}</strong>
      <span>{render.status}</span>
      {render.total > 0 && <><Progress label="전체 렌더 진행률" value={percent(render.done, render.total)} />
        <small>{count(render.done)} / {count(render.total)} 프레임{render.fps > 0 ? ` · ${render.fps.toFixed(1)} fps` : ""}</small></>}
      <div className="actions">
        <Button size="sm" variant="secondary" onPress={() => navigate("monitor")}>모니터</Button>
        {data.connected && <Button size="sm" variant="ghost" onPress={() => void act({ action: "disconnect" })}><Square size={13} /> 연결 종료</Button>}
        {!data.locked && render.output && <Button size="sm" variant="ghost" onPress={() => void act({ action: "open_location", location: "output_directory" })}>
          <FolderOpen size={14} /> 출력 폴더</Button>}
      </div>
    </div>
  </div>;
}

function SplitHandle({ axis, value, update }: { axis: "height" | "width"; value: number; update: (value: number) => void }) {
  const start = useRef<{ position: number; value: number } | undefined>(undefined);
  const verticalAxis = axis === "height";
  const limit = (value: number) => Math.min(verticalAxis ? 420 : 440, Math.max(verticalAxis ? 140 : 185, value));
  return <Button variant="ghost" isIconOnly aria-label={verticalAxis ? "하단 패널 높이 조절" : "작업 PC 패널 너비 조절"}
    className={`reference-split split-${axis}`} onPointerDown={(event) => {
      if (event.button !== 0) return;
      start.current = { position: verticalAxis ? event.clientY : event.clientX, value };
      event.currentTarget.setPointerCapture(event.pointerId);
    }} onPointerMove={(event) => {
      if (!start.current) return;
      const position = verticalAxis ? event.clientY : event.clientX;
      update(limit(start.current.value + start.current.position - position));
    }} onPointerUp={() => { start.current = undefined; }} onPointerCancel={() => { start.current = undefined; }}
    onKeyDown={(event) => {
      const smaller = verticalAxis ? "ArrowDown" : "ArrowRight";
      const larger = verticalAxis ? "ArrowUp" : "ArrowLeft";
      if (event.key !== smaller && event.key !== larger) return;
      event.preventDefault();
      update(limit(value + (event.key === larger ? 16 : -16)));
    }}><Graphic src={verticalAxis ? horizontal : vertical} /></Button>;
}
