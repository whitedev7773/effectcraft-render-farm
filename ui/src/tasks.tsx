import { Accordion, Button, Card, Chip, Slider, Spinner, Table, ToggleButton, ToggleButtonGroup } from "@heroui/react";
import { ArrowDown, ArrowUp, ChevronDown, FolderOpen, Grid2X2, List, Plus, Settings2, Trash2, WandSparkles } from "lucide-react";
import { useState } from "react";
import { count, filename, isActive, percent, type Action, type Composition, type Distribution,
  type ImportDraft, type RenderDefaults, type Snapshot, type Task, type TaskOptions } from "./api";
import { Choice, DataTable, Dialog, Empty, Field, FrameImage, IconButton, Notice, Numeric, PageHeading, Panel, Progress, Search, StatusChip, Toggle } from "./components";
import queueIcon from "../../assets/ui/figma/nav-list.svg";
import emptyQueue from "../../assets/ui/figma/queue-empty.svg";

const filters = [
  { value: "all", label: "모든 상태" }, { value: "Pending", label: "대기" }, { value: "active", label: "진행 중" },
  { value: "Completed", label: "완료" }, { value: "attention", label: "확인 필요" },
];

export function queueMatches(task: Task, search: string, filter: string) {
  const matches = filter === "all" || task.status === filter || filter === "active" && isActive(task.status)
    || filter === "attention" && ["Failed", "Interrupted"].includes(task.status);
  return matches && [task.project, task.composition.name, task.final_video].some((value) => value.toLowerCase().includes(search.trim().toLowerCase()));
}

export function QueuePage({ data, act, openTask }: { data: Snapshot; act: Action; openTask: (id: number) => void }) {
  const [search, setSearch] = useState("");
  const [filter, setFilter] = useState("all");
  const tasks = data.tasks.filter((task) => queueMatches(task, search, filter));
  const pending = data.tasks.filter((task) => task.status === "Pending").map((task) => task.id);
  const add = () => void act({ action: "open_project" });
  const controls = (task: Task) => {
    const position = pending.indexOf(task.id);
    return <div className="row-actions">
      {position >= 0 && <>
        <IconButton label={`${task.composition.name} 순서 올리기`} icon={ArrowUp} isDisabled={position === 0}
          onPress={() => void act({ action: "move_task", id: task.id, direction: -1 })} />
        <IconButton label={`${task.composition.name} 순서 내리기`} icon={ArrowDown} isDisabled={position === pending.length - 1}
          onPress={() => void act({ action: "move_task", id: task.id, direction: 1 })} />
      </>}
      <IconButton label={`${task.composition.name} 작업 상세`} icon={Settings2} onPress={() => openTask(task.id)} />
    </div>;
  };
  return <div className="farm-page">
    <PageHeading title="대기열 관리"
      description={`전체 ${count(data.tasks.length)}개 · 대기 ${count(pending.length)}개`}
      actions={<Button variant="primary" isDisabled={data.locked} onPress={add}><Plus size={16} /> 프로젝트 추가</Button>} />
    <Panel title="작업 목록" icon={queueIcon}
      action={<ToggleButtonGroup className="farm-segment" aria-label="대기열 보기" selectionMode="single" disallowEmptySelection
        selectedKeys={new Set([data.settings.queue_cards ? "cards" : "list"])} onSelectionChange={(keys) => {
          const value = [...keys][0];
          if (value) void act({ action: "queue_view", cards: value === "cards" });
        }}>
        <ToggleButton id="cards" isIconOnly aria-label="카드 보기"><Grid2X2 size={16} /></ToggleButton>
        <ToggleButton id="list" isIconOnly aria-label="목록 보기"><List size={16} /></ToggleButton>
      </ToggleButtonGroup>}>
      <div className="toolbar">
        <Search label="프로젝트, 컴포지션 또는 출력 검색" value={search} onChange={setSearch} />
        <Choice label="작업 상태 필터" value={filter} options={filters} onChange={setFilter} compact />
        {tasks.length !== data.tasks.length && <span className="toolbar-count">{count(tasks.length)}개 표시</span>}
      </div>
      {!tasks.length ? <Empty icon={emptyQueue} title={data.tasks.length ? "일치하는 작업이 없습니다" : "대기열이 비어 있습니다"}>
        {data.tasks.length ? "검색어나 작업 상태 필터를 변경하세요." : ".ecproj 파일을 끌어오거나 프로젝트를 추가하세요."}
      </Empty> : data.settings.queue_cards ? <div className="queue-cards">
        {tasks.map((task) => <Card key={task.id} className="queue-card">
          <FrameImage preview={task.preview} empty="프레임 대기 중" />
          <Card.Content>
            <div className="queue-card-title"><strong title={task.composition.name}>{task.composition.name}</strong><StatusChip task={task} /></div>
            <p title={task.project}>{filename(task.project)}</p>
            <TaskProgress task={task} />
          </Card.Content>
          <Card.Footer>
            <span className="muted truncate" title={task.final_video}>{filename(task.final_video)}</span>{controls(task)}
          </Card.Footer>
        </Card>)}
      </div> : <DataTable label="렌더 대기열" className="managed-queue-table">
            <Table.Header>
              <Table.Column isRowHeader>컴포지션</Table.Column><Table.Column>상태</Table.Column><Table.Column>진행률</Table.Column>
              <Table.Column>MP4 출력</Table.Column><Table.Column>작업</Table.Column>
            </Table.Header>
            <Table.Body items={tasks}>
              {(task) => <Table.Row id={task.id}>
                <Table.Cell><Button variant="ghost" className="table-name" size="sm" onPress={() => openTask(task.id)}>
                  <span className="row-copy"><strong>{task.composition.name}</strong><small>{filename(task.project)}</small></span>
                </Button></Table.Cell>
                <Table.Cell><StatusChip task={task} /></Table.Cell>
                <Table.Cell className="table-progress"><TaskProgress task={task} /></Table.Cell>
                <Table.Cell><span className="table-path truncate" title={task.final_video}>{filename(task.final_video)}</span></Table.Cell>
                <Table.Cell>{controls(task)}</Table.Cell>
              </Table.Row>}
            </Table.Body>
      </DataTable>}
    </Panel>
  </div>;
}

function TaskProgress({ task }: { task: Task }) {
  const total = (task.range_end || task.composition.frames) - task.range_start;
  const done = task.status === "Completed" ? total : task.done;
  return <div className="task-progress">
    <div><span>{count(done)} / {count(total)} 프레임</span><strong>{Math.round(percent(done, total))}%</strong></div>
    <Progress label={`${task.composition.name} 진행률`} value={percent(done, total)} />
  </div>;
}

function CompositionMeta({ composition }: { composition: Composition }) {
  const fps = composition.rate.den ? composition.rate.num / composition.rate.den : 0;
  return <div className="composition-meta">
    <Chip size="sm" variant="soft">{composition.width} × {composition.height}</Chip>
    <Chip size="sm" variant="soft">{Number(fps.toFixed(3))} fps</Chip>
    <Chip size="sm" variant="soft">{count(composition.frames)} 프레임</Chip>
  </div>;
}

function TaskFields({ options, update, composition, act, disabled }: {
  options: TaskOptions; update: (next: TaskOptions) => void; composition: Composition; act: Action; disabled?: boolean;
}) {
  const start = options.range_start;
  const end = options.range_end || composition.frames;
  async function pick() {
    const path = await act<string | null>({ action: "pick_output", suggested: options.final_video });
    if (path) update({ ...options, final_video: path });
  }
  return <div className="stack form-stack">
    <Field label="최종 MP4 저장 경로" value={options.final_video} disabled={disabled}
      onChange={(final_video) => update({ ...options, final_video })} extra={
        <IconButton icon={FolderOpen} label="MP4 저장 위치 선택" variant="secondary" isDisabled={disabled} onPress={() => void pick()} />
      } />
    <div className="range-block">
      <div className="range-label"><strong>렌더 범위</strong><span>{count(end - start)} 프레임 · 끝 프레임 제외</span></div>
      <Slider aria-label="렌더 범위" value={[start, end]} minValue={0} maxValue={composition.frames} isDisabled={disabled}
        onChange={(values) => {
          if (Array.isArray(values) && values[0] < values[1]) update({ ...options, range_start: values[0], range_end: values[1] });
        }}>
        <Slider.Track><Slider.Fill /><Slider.Thumb index={0} aria-label="범위 시작" /><Slider.Thumb index={1} aria-label="범위 끝" /></Slider.Track>
      </Slider>
      <div className="form-grid">
        <Numeric label="시작 프레임" min={0} max={end - 1} value={start} disabled={disabled} onChange={(range_start) => update({ ...options, range_start })} />
        <Numeric label="끝 프레임" min={start + 1} max={composition.frames} value={end} disabled={disabled} onChange={(range_end) => update({ ...options, range_end })} />
      </div>
      <div className="actions">
        <Button size="sm" variant="ghost" isDisabled={disabled} onPress={() => update({ ...options, range_start: composition.work_start, range_end: composition.work_end })}>컴포지션 작업 영역</Button>
        <Button size="sm" variant="ghost" isDisabled={disabled} onPress={() => update({ ...options, range_start: 0, range_end: composition.frames })}>전체 프레임</Button>
      </div>
    </div>
    <Accordion className="settings-accordion">
      <Accordion.Item>
        <Accordion.Heading><Accordion.Trigger><Settings2 size={16} /> 렌더 옵션 <Accordion.Indicator><ChevronDown size={16} /></Accordion.Indicator></Accordion.Trigger></Accordion.Heading>
        <Accordion.Panel><Accordion.Body><RenderFields value={options} update={(render) => update({ ...options, ...render })} disabled={disabled} act={act} /></Accordion.Body></Accordion.Panel>
      </Accordion.Item>
    </Accordion>
  </div>;
}

export function RenderFields({ value, update, disabled, act }: {
  value: RenderDefaults; update: (value: RenderDefaults) => void; disabled?: boolean; act: Action;
}) {
  const r = value.render;
  const change = <K extends keyof typeof r>(key: K, next: (typeof r)[K]) => update({ ...value, render: { ...r, [key]: next } });
  async function findFfmpeg() {
    const ffmpeg = await act<string | null>({ action: "pick_ffmpeg" });
    if (ffmpeg) change("ffmpeg", ffmpeg);
  }
  return <div className="stack form-stack">
    <div className="form-grid render-toggles">
      <Toggle label="자동 최적화" enabled={r.automatic} disabled={disabled} onChange={(next) => change("automatic", next)} />
      <Toggle label="GPU 렌더링" enabled={value.gpu || r.automatic} disabled={disabled || r.automatic} onChange={(gpu) => update({ ...value, gpu })} />
      <Toggle label="Draft 품질" enabled={r.draft} disabled={disabled} onChange={(next) => change("draft", next)} />
      <Toggle label="오디오 포함" enabled={r.audio} disabled={disabled} onChange={(next) => change("audio", next)} />
      <Toggle label="하드웨어 인코딩" enabled={r.nvenc} disabled={disabled} onChange={(next) => change("nvenc", next)} />
      <Toggle label="라이브 미리보기" enabled={r.previews} disabled={disabled} onChange={(next) => change("previews", next)} />
      <Toggle label="처리 속도에 맞춰 구간 분할" enabled={r.adaptive} disabled={disabled} onChange={(next) => change("adaptive", next)} />
      <Toggle label="수신 프레임 디스크 동기화" enabled={r.durable_writes} disabled={disabled} onChange={(next) => change("durable_writes", next)} />
    </div>
    <div className="form-grid">
      <Numeric label="MP4 비트레이트 (kbps)" min={100} max={200000} value={r.video_bitrate} disabled={disabled} onChange={(next) => change("video_bitrate", next)} />
      <Numeric label="미리보기 주기 (초)" min={1} max={30} value={r.preview_seconds} disabled={disabled || !r.previews} onChange={(next) => change("preview_seconds", next)} />
      <Choice label="구간 분배" value={value.distribution} disabled={disabled}
        options={[{ value: "auto", label: "자동" }, { value: "frames", label: "프레임 수" }, { value: "percent", label: "비율" }]}
        onChange={(distribution) => update({ ...value, distribution: distribution as Distribution })} />
      {value.distribution === "frames" && <Numeric label="구간당 프레임" min={1} max={100000} value={value.chunk_frames} disabled={disabled} onChange={(chunk_frames) => update({ ...value, chunk_frames })} />}
      {value.distribution === "percent" && <Numeric label="구간 비율 (%)" min={0.01} max={100} step={0.01} value={value.chunk_percent} disabled={disabled} onChange={(chunk_percent) => update({ ...value, chunk_percent })} />}
      <Numeric label="동시 참여 PC 수" min={1} max={64} value={r.automatic ? 64 : value.max_workers} disabled={disabled || r.automatic} onChange={(max_workers) => update({ ...value, max_workers })} />
      <Numeric label="목표 구간 시간 (초)" min={5} max={300} value={r.target_seconds} disabled={disabled} onChange={(next) => change("target_seconds", next)} />
      <Numeric label="구간 최대 시도" min={1} max={10} value={r.retries} disabled={disabled} onChange={(next) => change("retries", next)} />
      <Numeric label="프레임 감지 주기 (ms)" min={50} max={2000} value={r.poll_ms} disabled={disabled} onChange={(next) => change("poll_ms", next)} />
    </div>
    <Field label="FFmpeg 경로" placeholder="비우면 자동 감지" value={r.ffmpeg} disabled={disabled} onChange={(next) => change("ffmpeg", next)}
      extra={<IconButton icon={FolderOpen} label="FFmpeg 실행 파일 선택" variant="secondary" isDisabled={disabled} onPress={() => void findFfmpeg()} />} />
  </div>;
}

export function ImportDialog({ draft, act, done, nativeBusy }: { draft: ImportDraft; act: Action; done: () => void; nativeBusy: boolean }) {
  const comp = draft.selected === null ? undefined : draft.compositions[draft.selected];
  const [edited, setOptions] = useState<TaskOptions>();
  const options = edited ?? draft.options;
  const [sending, setSending] = useState(false);
  const locked = sending || draft.preview_busy;
  const close = async () => { await act({ action: "close_import" }); };
  async function submit(commit: boolean) {
    setSending(true);
    const saved = await act({ action: "configure_import", options, commit, preview: !commit }, commit ? "작업을 대기열에 추가했습니다." : undefined);
    setSending(false);
    if (commit && saved !== undefined) done();
  }
  return <Dialog title="프로젝트 추가" description={filename(draft.project)} wide locked={locked} onClose={close}
    footer={(dismiss) => <>
      <Button variant="ghost" onPress={dismiss} isDisabled={locked}>취소</Button>
      <Button variant="primary" isDisabled={!comp || locked || !options.final_video.trim()} onPress={() => void submit(true)}>
        <Plus size={17} /> 대기열에 추가
      </Button>
    </>}>
    {draft.loading && <div className="form-loading"><Spinner size="sm" /> 컴포지션을 읽고 있습니다.</div>}
    {!draft.loading && !draft.compositions.length && !draft.error && <Notice title="렌더 가능한 컴포지션이 없습니다." />}
    {!draft.loading && draft.compositions.length > 0 && <div className="composition-summary">
      <Choice label="렌더할 컴포지션" value={draft.selected === null ? null : String(draft.selected)}
      options={draft.compositions.map((item, index) => ({ value: String(index), label: item.name }))}
      onChange={(index) => {
        setOptions(undefined);
        void act({ action: "select_composition", index: Number(index) });
      }} disabled={locked} />
      {comp && <CompositionMeta composition={comp} />}
    </div>}
    {comp && <>
      <TaskFields options={options} update={setOptions} composition={comp} act={act} disabled={locked} />
      <div className="actions">
        <Button variant="secondary" size="sm" isDisabled={locked || nativeBusy} onPress={() => void submit(false)}>
          {draft.preview_busy ? <Spinner size="sm" /> : <WandSparkles size={15} />} 시험 렌더
        </Button>
      </div>
      {draft.preview && <FrameImage preview={draft.preview} />}
    </>}
    {draft.error && <Notice title="프로젝트를 확인해 주세요" danger>{draft.error}</Notice>}
  </Dialog>;
}

export function TaskDialog({ task, act, close, active }: { task: Task; act: Action; close: () => void; active: boolean }) {
  const [options, setOptions] = useState<TaskOptions>(() => ({
    final_video: task.final_video, range_start: task.range_start, range_end: task.range_end || task.composition.frames,
    distribution: task.distribution, chunk_frames: task.chunk_frames, chunk_percent: task.chunk_percent,
    max_workers: task.max_workers, gpu: task.gpu, render: task.render,
  }));
  const [confirm, setConfirm] = useState(false);
  const [busy, setBusy] = useState(false);
  const editable = !active && !isActive(task.status) && task.status !== "Completed";
  const retry = task.status === "Failed" || task.status === "Interrupted";
  async function save() {
    setBusy(true);
    const result = await act({ action: "save_task", id: task.id, options, retry }, retry ? "작업을 재대기했습니다." : "작업 설정을 저장했습니다.");
    if (result === undefined) setBusy(false);
    else close();
  }
  async function remove() {
    setBusy(true);
    const result = await act({ action: "remove_task", id: task.id }, "대기열에서 작업을 제거했습니다.");
    if (result === undefined) setBusy(false);
    else close();
  }
  if (confirm) return <Dialog title="작업을 제거할까요?" description={task.composition.name} locked={busy} onClose={() => setConfirm(false)}
    footer={(dismiss) => <><Button variant="secondary" isDisabled={busy} onPress={dismiss}>돌아가기</Button>
      <Button variant="danger" isDisabled={busy} onPress={() => void remove()}><Trash2 size={16} /> 제거</Button></>}>
    <p>대기열에서 이 항목을 제거합니다. 저장된 프레임과 MP4 파일은 보관됩니다.</p>
  </Dialog>;
  return <Dialog title={task.composition.name} description={filename(task.project)} onClose={close} wide locked={busy}
    footer={(dismiss) => <>
      {!active && !isActive(task.status) && <Button variant="ghost" className="remove-button" isDisabled={busy} onPress={() => setConfirm(true)}><Trash2 size={16} /> 제거</Button>}
      <Button variant="secondary" isDisabled={busy} onPress={dismiss}>{editable ? "취소" : "닫기"}</Button>
      {editable && <Button variant="primary" isDisabled={busy} onPress={() => void save()}>{retry ? "저장 후 재대기" : "설정 저장"}</Button>}
    </>}>
    <div className="composition-summary">
      <div className="actions"><StatusChip task={task} /><CompositionMeta composition={task.composition} /></div>
      <TaskProgress task={task} />
      {task.detail && <p className="muted">{task.detail}</p>}
    </div>
    {task.preview && <FrameImage preview={task.preview} />}
    <TaskFields options={options} update={setOptions} composition={task.composition} act={act} disabled={!editable || busy} />
    {task.status === "Completed" && <Button variant="outline" onPress={() => void act({ action: "open_location", location: "output_directory", task: task.id })}>
      <FolderOpen size={16} /> 출력 폴더 열기
    </Button>}
  </Dialog>;
}
