import { Button, Chip, Slider, Table, toast } from "@heroui/react";
import { Activity, ArrowRight, ChartNoAxesColumnIncreasing, Copy, FolderOpen, ScanLine, Terminal } from "lucide-react";
import type { CSSProperties } from "react";
import { useState } from "react";
import { count, elapsed, message, percent, sizeMiB, type Action, type Page, type Snapshot, type Worker } from "./api";
import { Choice, DataTable, Dialog, Empty, FrameImage, Graphic, Notice, Numeric, PageHeading, Panel, Progress, Search, Toggle } from "./components";
import pcIcon from "../../assets/ui/figma/pc-display.svg";
import emptyPC from "../../assets/ui/figma/pc-link-slash.svg";
import monitorIcon from "../../assets/ui/figma/nav-monitor.svg";
import searchEmpty from "../../assets/ui/figma/queue-empty.svg";

type Props = { data: Snapshot; act: Action };
export function MonitorPage({ data, act, navigate, openWorker, inspect }: Props & {
  navigate: (page: Page) => void; openWorker: (id: number) => void; inspect: () => void;
}) {
  const r = data.render_state;
  const value = percent(r.done, r.total);
  const eta = r.total > r.done && r.fps > 0 && (data.dispatch || !data.settings.host && data.connected) ? (r.total - r.done) / r.fps : null;
  const timeline = r.timeline.length ? r.timeline : Array<number>(48).fill(0);
  return <div className="farm-page">
    <PageHeading title="렌더 모니터" description={r.status || undefined} actions={data.settings.host &&
      <Button variant="secondary" onPress={inspect} isDisabled={!data.inspect.can_load && !data.inspect.can_render}><ScanLine size={16} /> 프레임 탐색</Button>} />
    <div className="monitor-grid">
      <Panel title="라이브 미리보기" icon={monitorIcon} description={r.comp ?? undefined} className="monitor-preview-panel">
        <FrameImage preview={r.preview} empty="첫 프레임이 수신되면 여기에 표시됩니다." />
      </Panel>
      <Panel title="렌더 진행률" icon={Activity} className="monitor-progress-panel">
        <div className="progress-readout">
          <div className="progress-summary"><strong>{value.toFixed(1)}<span>%</span></strong><small>{count(r.done)} / {count(r.total)} 프레임</small></div>
          <Progress label="전체 렌더 진행률" value={value} />
        </div>
        <dl className="render-metrics">
          <div><dt>경과 시간</dt><dd>{elapsed(r.elapsed)}</dd></div>
          <div><dt>처리 속도</dt><dd>{r.fps > 0 ? `${r.fps.toFixed(2)} fps` : "—"}</dd></div>
          <div><dt>예상 잔여</dt><dd>{elapsed(eta)}</dd></div>
          <div><dt>MP4 출력</dt><dd>{r.encoding ? "인코딩 중" : r.total > 0 && r.done === r.total ? "프레임 완료" : "—"}</dd></div>
        </dl>
        {r.profile && <p className="muted">{r.profile}</p>}
        {r.local_range && !data.settings.host && <p className="muted">할당 구간 {count(r.local_range[0])}–{count(r.local_range[1])}</p>}
      </Panel>
    </div>
    {data.settings.host && <Panel title="프레임 타임라인" icon={ChartNoAxesColumnIncreasing}
      action={r.frames && <Button size="sm" variant="ghost" onPress={() => void act({ action: "open_location", location: "frame_directory" })}><FolderOpen size={16} /> 폴더 열기</Button>}>
      <div className="timeline" style={{ "--segments": timeline.length } as CSSProperties}
        role="img" aria-label={`완료 ${count(r.done)} / ${count(r.total)} 프레임, 실패 ${count(r.failures)}구간`}>
        {timeline.map((state, index) => <span key={index} data-state={state} />)}
      </div>
      <div className="timeline-legend">{["대기", "렌더 중", "완료", "실패"].map((label, index) =>
        <span key={label}><i data-state={index} />{label}</span>)}</div>
    </Panel>}
    {data.settings.host && <WorkerList data={data} act={act} openWorker={openWorker} />}
    <Panel title="최근 활동" icon={Terminal} action={<Button variant="ghost" size="sm" onPress={() => navigate("logs")}>전체 기록 <ArrowRight size={15} /></Button>}>
      <LogEntries entries={r.messages.filter((line) => !line.includes("] [DEBUG]")).slice(-5).reverse()} />
    </Panel>
  </div>;
}

export function WorkerList({ data, act, openWorker }: Props & { openWorker: (id: number) => void }) {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState("all");
  const workers = data.workers.filter((worker) => `${worker.label} ${worker.ip}`.toLowerCase().includes(query.toLowerCase().trim())
    && (filter === "all" || (filter === "online" ? worker.connected : worker.connected && worker.enabled)));
  return <Panel title="작업 PC" icon={pcIcon}>
    {data.workers.length > 0 && <div className="toolbar">
      <Search label="이름 또는 IP 검색" value={query} onChange={setQuery} />
      <Choice label="PC 상태 필터" value={filter} options={[{ value: "all", label: "모든 PC" }, { value: "online", label: "연결됨" }, { value: "enabled", label: "렌더 참여" }]} onChange={setFilter} compact />
    </div>}
    {workers.length ? <DataTable label="작업 PC 목록">
          <Table.Header><Table.Column isRowHeader>PC 이름</Table.Column><Table.Column>상태</Table.Column><Table.Column>현재 구간</Table.Column><Table.Column>속도</Table.Column><Table.Column>참여</Table.Column></Table.Header>
          <Table.Body items={workers}>
            {(worker) => <Table.Row id={worker.id}>
              <Table.Cell><Button variant="ghost" className="table-name" size="sm" onPress={() => openWorker(worker.id)}>
                <Graphic src={pcIcon} /><span className="row-copy"><strong>{worker.label}</strong><small className="mono">{worker.ip}</small></span>
              </Button></Table.Cell>
              <Table.Cell><WorkerStatus worker={worker} /></Table.Cell>
              <Table.Cell className="mono">{worker.range ? `${count(worker.range[0])}–${count(worker.range[1])}` : "—"}</Table.Cell>
              <Table.Cell className="mono">{worker.fps > 0 ? `${worker.fps.toFixed(2)} fps` : "—"}</Table.Cell>
              <Table.Cell><Toggle label={`${worker.label} 렌더 참여`} enabled={worker.enabled}
                disabled={!worker.connected || data.stopping} onChange={(enabled) => void act({ action: "worker_participation", id: worker.id, enabled })} /></Table.Cell>
            </Table.Row>}
          </Table.Body>
    </DataTable> : <Empty icon={data.workers.length ? searchEmpty : emptyPC} title={data.workers.length ? "검색 결과가 없습니다" : "연결된 작업 PC 없음"}>
      {data.workers.length ? "검색어 또는 상태 필터를 변경하세요." : "호스트를 열고 다른 PC에서 호스트 주소로 연결하세요."}
    </Empty>}
  </Panel>;
}

function WorkerStatus({ worker }: { worker: Worker }) {
  return <Chip size="sm" variant="soft" color={worker.connected ? worker.ready ? "success" : "warning" : "default"}>
    <Chip.Label>{worker.connected ? worker.ready ? "연결됨" : "준비 중" : "오프라인"}</Chip.Label>
  </Chip>;
}

export function WorkerDialog({ worker, data, act, close }: Props & { worker: Worker; close: () => void }) {
  const values = [
    ["주소", worker.ip], ["완료 프레임", count(worker.completed)], ["처리 속도", `${worker.fps.toFixed(2)} fps`],
    ["전송량", `${sizeMiB(worker.bytes)} MiB`], ["전송 속도", worker.seconds > 0 ? `${sizeMiB(worker.bytes / worker.seconds)} MiB/s` : "—"],
    ["현재 구간 경과", elapsed(worker.elapsed)],
  ];
  return <Dialog title={worker.label} description={worker.profile || undefined} onClose={close} wide
    footer={<Button variant="secondary" onPress={close}>닫기</Button>}>
    <div className="actions"><WorkerStatus worker={worker} />{worker.range && <span className="muted">구간 {count(worker.range[0])}–{count(worker.range[1])}</span>}</div>
    <FrameImage preview={worker.preview} />
    <dl className="detail-grid">{values.map(([label, value]) => <div key={label}><dt>{label}</dt><dd>{value}</dd></div>)}</dl>
    <Toggle label="이 PC를 렌더링에 사용" enabled={worker.enabled} disabled={!data.settings.host || !worker.connected || data.stopping}
      onChange={(enabled) => void act({ action: "worker_participation", id: worker.id, enabled })} />
  </Dialog>;
}

const logClock = new Intl.DateTimeFormat("en-GB", { hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23" });

function splitLog(line: string) {
  const match = /^\[(\d+\.\d+) UTC\] \[([A-Z]+)\] \[([^\]]+)\] (.*)$/.exec(line);
  if (!match) return { time: "", level: "INFO", category: "", text: line };
  const time = logClock.format(Number(match[1]) * 1000);
  return { time, level: match[2], category: match[3], text: match[4] };
}

function LogEntries({ entries }: { entries: string[] }) {
  if (!entries.length) return <Empty icon={searchEmpty} title="표시할 작업 기록 없음" />;
  const rows = entries.map((line, index) => ({ id: `${index}-${line}`, ...splitLog(line) }));
  return <DataTable label="이벤트 로그" className="log-table">
    <Table.Header>
      <Table.Column className="log-time">시간</Table.Column>
      <Table.Column className="log-level-column">수준</Table.Column>
      <Table.Column className="log-category">범주</Table.Column>
      <Table.Column isRowHeader>메시지</Table.Column>
    </Table.Header>
    <Table.Body items={rows}>
      {({ id, time, level, text, category }) => <Table.Row id={id}>
        <Table.Cell className="log-time"><time className="mono">{time || "—"}</time></Table.Cell>
        <Table.Cell><span className="log-level mono" data-level={level}>{level}</span></Table.Cell>
        <Table.Cell className="log-category"><span className="mono" title={category || undefined}>{category || "—"}</span></Table.Cell>
        <Table.Cell className="log-text">{text}</Table.Cell>
      </Table.Row>}
    </Table.Body>
  </DataTable>;
}

export function LogsPage({ data, act }: Props) {
  const [query, setQuery] = useState("");
  const [level, setLevel] = useState("important");
  const entries = data.render_state.messages.filter((line) => {
    const entry = splitLog(line);
    return line.toLowerCase().includes(query.trim().toLowerCase())
      && (level === "all" || (level === "important" ? entry.level !== "DEBUG" : entry.level === level));
  }).reverse();
  async function copy() {
    try {
      await navigator.clipboard.writeText([...entries].reverse().join("\n"));
      toast.success("표시 중인 기록을 복사했습니다.");
    } catch (error) {
      toast.danger(message(error));
    }
  }
  return <div className="farm-page">
    <PageHeading title="작업 기록" actions={
      <Button variant="secondary" onPress={() => void act({ action: "open_location", location: "log_directory" })}><FolderOpen size={16} /> 로그 폴더</Button>} />
    <Panel title="이벤트 로그" icon={Terminal} description={`${count(entries.length)}개 기록 · 최근 순`}
      action={<Button size="sm" variant="ghost" onPress={() => void copy()} isDisabled={entries.length === 0}><Copy size={16} /> 복사</Button>}>
      <div className="toolbar">
        <Search label="작업 기록 검색" value={query} onChange={setQuery} />
        <Choice label="로그 수준" value={level} onChange={setLevel} compact options={[
          { value: "important", label: "일반 기록" }, { value: "all", label: "전체 (DEBUG 포함)" },
          ...["ERROR", "WARN", "INFO", "DEBUG"].map((value) => ({ value, label: value })),
        ]} />
      </div>
      <LogEntries entries={entries} />
    </Panel>
  </div>;
}

export function InspectDialog({ data, act, close }: Props & { close: () => void }) {
  const inspect = data.inspect;
  const [frame, setFrame] = useState(inspect.start);
  return <Dialog title="프레임 탐색" description="완료 프레임을 열거나 선택한 프레임을 시험 렌더합니다." onClose={close} wide
    footer={<>
      <Button variant="secondary" isDisabled={!inspect.can_load || inspect.busy} onPress={() => void act({ action: "inspect", frame, render: false })}>완료 프레임 보기</Button>
      <Button variant="primary" isDisabled={!inspect.can_render || inspect.busy} onPress={() => void act({ action: "inspect", frame, render: true })}>시험 렌더</Button>
    </>}>
    <FrameImage preview={inspect.preview} />
    <div className="inspect-input">
      {inspect.end - inspect.start > 1 && <Slider aria-label="프레임 위치" minValue={inspect.start} maxValue={inspect.end - 1}
        value={frame} onChange={(value) => setFrame(Number(value))}><Slider.Track><Slider.Fill /><Slider.Thumb /></Slider.Track></Slider>}
      <Numeric label="프레임 번호" min={inspect.start} max={inspect.end - 1} value={frame} onChange={setFrame} />
    </div>
    {inspect.status && <p role="status" className="muted">{inspect.status}</p>}
    {!inspect.can_load && !inspect.can_render && <Notice title="사용 가능한 프레임이 없습니다." />}
  </Dialog>;
}
