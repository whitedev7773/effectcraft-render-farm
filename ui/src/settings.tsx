import { Button, Chip, Separator, Spinner, Tabs, ToggleButton, ToggleButtonGroup } from "@heroui/react";
import { ArrowDownToLine, ArrowUpRight, Check, Cpu, Eye, EyeOff, FolderCog, Globe2, Network, Radio, RefreshCw, Server, SunMoon, Terminal } from "lucide-react";
import { useState } from "react";
import { sizeMiB, type Action, type Appearance, type Page, type RenderDefaults, type Snapshot, type Theme } from "./api";
import { Choice, Dialog, Empty, Field, Graphic, IconButton, Notice, PageHeading, Panel, Toggle } from "./components";
import { RenderFields } from "./tasks";
import { WorkerList } from "./operations";
import serverIcon from "../../assets/ui/figma-welcome/server.svg";
import nodeIcon from "../../assets/ui/figma-welcome/node.svg";
import pcIcon from "../../assets/ui/figma/pc-display.svg";
import emptyPC from "../../assets/ui/figma/pc-link-slash.svg";
import settingsIcon from "../../assets/ui/figma/nav-settings.svg";

type Props = { data: Snapshot; act: Action };

export function NodesPage({ data, act, openWorker }: Props & { openWorker: (id: number) => void }) {
  const s = data.settings;
  const [selectedHost, chooseHost] = useState<string | null>(null);
  const key = JSON.stringify([s.host, s.host_address, s.client_address, s.token, s.local_worker]);
  const connected = data.workers.filter((worker) => worker.connected);
  return <div className="farm-page">
    <PageHeading title="연결 및 작업 PC" />
    <div className="connection-grid">
      <ConnectionForm key={key} data={data} act={act} selectedHost={selectedHost} clearHost={() => chooseHost(null)} />
      <Panel title={s.host ? "호스트 세션" : "주변 호스트"} icon={s.host ? pcIcon : Radio}
        action={!s.host && <IconButton label="호스트 다시 검색" icon={RefreshCw} isDisabled={data.discovering || data.locked} onPress={() => void act({ action: "discover" })} />}>
        {s.host ? <>
          <dl className="detail-grid connection-metrics">
            <div><dt>수신 주소</dt><dd className="mono">{s.host_address}</dd></div>
            <div><dt>연결된 PC</dt><dd>{connected.length}대</dd></div>
            <div><dt>렌더 참여 PC</dt><dd>{connected.filter((worker) => worker.enabled).length}대</dd></div>
          </dl>
          {data.locked && <p className="muted">{data.render_state.status}</p>}
        </> : <Discovery data={data} selectedHost={selectedHost} chooseHost={chooseHost} />}
      </Panel>
    </div>
    {s.host && <WorkerList data={data} act={act} openWorker={openWorker} />}
  </div>;
}

function ConnectionForm({ data, act, selectedHost, clearHost }: Props & { selectedHost: string | null; clearHost: () => void }) {
  const s = data.settings;
  const [host, setHost] = useState(s.host);
  const [addresses, setAddresses] = useState({ host: s.host_address, worker: s.client_address });
  const [token, setToken] = useState(s.token);
  const [localWorker, setLocalWorker] = useState(s.local_worker);
  const [reveal, setReveal] = useState(false);
  const [saving, setSaving] = useState(false);
  const role = host ? "host" : "worker";
  const address = host ? addresses.host : selectedHost ?? addresses.worker;
  const disabled = data.locked || saving;
  const dirty = host !== s.host || address !== (host ? s.host_address : s.client_address) || token !== s.token || localWorker !== s.local_worker;
  async function save() {
    setSaving(true);
    const saved = await act({ action: "set_connection", host, address, token, local_worker: localWorker }, "연결 설정을 저장했습니다.");
    if (saved !== undefined) clearHost();
    setSaving(false);
  }
  return <Panel title="이 PC의 연결 설정" icon={Network} footer={<>
    {data.locked && <span className="muted">연결 중에는 설정이 잠깁니다.</span>}
    <Button variant="primary" isDisabled={disabled || !dirty} onPress={() => void save()}>{saving ? <Spinner size="sm" /> : <Check size={16} />} 설정 저장</Button>
  </>}>
    <div className="stack form-stack">
      <ToggleButtonGroup className="farm-segment" aria-label="이 PC의 역할" selectionMode="single" disallowEmptySelection
        selectedKeys={new Set([role])} isDisabled={disabled} onSelectionChange={(keys) => {
          const selected = [...keys][0];
          if (selected) setHost(selected === "host");
        }}>
        <ToggleButton id="host"><Graphic src={serverIcon} /> 호스트</ToggleButton>
        <ToggleButton id="worker"><Graphic src={nodeIcon} /> 작업 PC</ToggleButton>
      </ToggleButtonGroup>
      <Field label={host ? "수신 주소" : "호스트 주소"} value={address} disabled={disabled}
        placeholder={host ? "0.0.0.0:9778" : "192.168.0.10:9778"} onChange={(value) => {
          if (!host) clearHost();
          setAddresses({ ...addresses, [role]: value });
        }} />
      <Field label="접속 암호" value={token} onChange={setToken} disabled={disabled} secret={!reveal} placeholder="8자 이상"
        extra={<IconButton label={reveal ? "암호 숨기기" : "암호 표시"} icon={reveal ? EyeOff : Eye} variant="secondary" onPress={() => setReveal(!reveal)} />} />
      <p className="field-help">{host ? "0.0.0.0은 이 PC의 모든 네트워크 인터페이스에서 연결을 받습니다." : "호스트와 동일한 접속 암호를 입력하세요."}</p>
      {host && <Toggle label="이 PC도 렌더링에 참여" description="호스트를 열면 이 PC의 작업자도 자동 연결합니다."
        enabled={localWorker} onChange={setLocalWorker} disabled={disabled} />}
    </div>
  </Panel>;
}

function Discovery({ data, selectedHost, chooseHost }: { data: Snapshot; selectedHost: string | null; chooseHost: (address: string) => void }) {
  const s = data.settings;
  return <div className="stack form-stack">
    {data.discovering && <div className="form-loading"><Spinner size="sm" /> LAN을 검색하는 중입니다.</div>}
    {data.hosts.map((host) => <Button key={host.address} variant="secondary" className="discovery-row" isDisabled={data.locked}
      onPress={() => chooseHost(host.address)}>
      <Server size={17} /><span className="row-copy"><strong>{host.name}</strong><small className="mono">{host.address}</small></span>
      {host.address === (selectedHost ?? s.client_address) ? <Check size={17} /> : <ArrowUpRight size={17} />}
    </Button>)}
    {!data.discovering && !data.hosts.length && <Empty title="검색된 호스트 없음" icon={emptyPC}>
      {data.discovery_status || "주소를 직접 입력해서 연결할 수 있습니다."}
    </Empty>}
  </div>;
}

export type SettingsTab = "appearance" | "render" | "about";

export function SettingsPage({ data, act, showUpdate, navigate, tab, changeTab }: Props & {
  showUpdate: (engine: boolean) => void; navigate: (page: Page) => void; tab: SettingsTab; changeTab: (tab: SettingsTab) => void;
}) {
  const s = data.settings;
  const renderKey = JSON.stringify([s.render, s.distribution, s.chunk_frames, s.chunk_percent, s.gpu, s.max_workers]);
  return <div className="farm-page">
    <PageHeading title="설정" actions={<>
      <Button variant="secondary" onPress={() => navigate("nodes")}><Network size={16} /> 연결 및 작업 PC</Button>
      <Button variant="secondary" onPress={() => navigate("logs")}><Terminal size={16} /> 작업 기록</Button>
      {s.host && <Button variant="secondary" onPress={() => navigate("queue_tools")}><FolderCog size={16} /> 대기열 관리</Button>}
    </>} />
    <Tabs className="farm-tabs" selectedKey={tab} onSelectionChange={(key) => changeTab(key as SettingsTab)}>
      <Tabs.ListContainer><Tabs.List aria-label="설정 범주">
        <Tabs.Tab id="appearance"><SunMoon size={16} /> 화면<Tabs.Indicator /></Tabs.Tab>
        <Tabs.Tab id="render"><Cpu size={16} /> 렌더 기본값<Tabs.Indicator /></Tabs.Tab>
        <Tabs.Tab id="about"><ArrowDownToLine size={16} /> 앱 및 업데이트<Tabs.Indicator /></Tabs.Tab>
      </Tabs.List></Tabs.ListContainer>
      <Tabs.Panel id="appearance" className="tab-panel"><AppearanceForm data={data} act={act} /></Tabs.Panel>
      <Tabs.Panel id="render" className="tab-panel"><RenderForm key={renderKey} data={data} act={act} /></Tabs.Panel>
      <Tabs.Panel id="about" className="tab-panel"><About data={data} act={act} showUpdate={showUpdate} /></Tabs.Panel>
    </Tabs>
  </div>;
}

function AppearanceForm({ data, act }: Props) {
  const { theme, reduced_motion, check_updates } = data.settings;
  const save = (options: Partial<Appearance>) => void act({ action: "set_appearance", theme, reduced_motion, check_updates, ...options });
  return <Panel title="테마 및 동작" icon={SunMoon} className="appearance-panel">
    <div className="stack form-stack appearance-fields">
      <Choice label="테마" value={theme} onChange={(value) => save({ theme: value as Theme })}
        options={[{ value: "dark", label: "다크" }, { value: "light", label: "라이트" }, { value: "system", label: "시스템 설정" }]} />
      <Toggle label="동작 줄이기" description="화면과 모달 전환에 사용되는 애니메이션을 줄입니다." enabled={reduced_motion} onChange={(value) => save({ reduced_motion: value })} />
      <Toggle label="Render Farm 업데이트 자동 확인" description="앱을 시작할 때 새 버전을 확인합니다." enabled={check_updates} onChange={(value) => save({ check_updates: value })} />
    </div>
  </Panel>;
}

function RenderForm({ data, act }: Props) {
  const s = data.settings;
  const initial: RenderDefaults = { render: s.render, gpu: s.gpu, distribution: s.distribution, chunk_frames: s.chunk_frames, chunk_percent: s.chunk_percent, max_workers: s.max_workers };
  const [value, update] = useState(initial);
  return <Panel title="렌더 기본값" description="새로 추가하는 작업에 적용됩니다." icon={settingsIcon} footer={
    <Button variant="primary" isDisabled={data.locked || JSON.stringify(initial) === JSON.stringify(value)}
      onPress={() => void act({ action: "set_render", ...value }, "렌더 기본값을 저장했습니다.")}><Check size={16} /> 기본값 저장</Button>}>
    <div className="stack">
      {data.locked && <Notice title="연결 중에는 렌더 기본값을 변경할 수 없습니다." />}
      <RenderFields value={value} update={update} act={act} disabled={data.locked} />
    </div>
  </Panel>;
}

function About({ data, act, showUpdate }: Props & { showUpdate: (engine: boolean) => void }) {
  const { engine, update } = data;
  return <div className="stack">
    <Panel title="EffectCraft 엔진" icon={Cpu} description={data.settings.cli || "EffectCraft 설치 경로를 찾고 있습니다."}
      action={<Chip color={engine.local ? "success" : "warning"} variant="soft"><Chip.Label>{engine.local ? `v${engine.local.replace(/^v/, "")}` : "설치 확인 필요"}</Chip.Label></Chip>}>
      <div className="stack form-stack">
        <p className="muted">{engine.missing || engine.status || "함께 렌더링하는 PC에서 호환되는 EffectCraft 버전을 사용하세요."}</p>
        <div className="actions">
          <Button variant="secondary" isDisabled={engine.checking || engine.installing || data.locked}
            onPress={() => void act({ action: "check_engine" })}><RefreshCw size={16} /> 설치 다시 확인</Button>
          <Button variant="ghost" onPress={() => void act({ action: "open_location", location: "engine_release" })}><Globe2 size={16} /> 공식 릴리스</Button>
          {engine.target && <Button variant="primary" onPress={() => showUpdate(true)}><ArrowDownToLine size={16} /> 업데이트 보기</Button>}
        </div>
      </div>
    </Panel>
    <Panel title="Render Farm" icon={ArrowDownToLine} description={update.status || undefined}
      action={<Chip variant="soft"><Chip.Label>v{data.version}</Chip.Label></Chip>}>
      <div className="actions">
        <Button variant="secondary" isDisabled={update.busy} onPress={() => void act({ action: "check_app" })}><RefreshCw size={16} /> 버전 확인</Button>
        <Button variant="ghost" onPress={() => void act({ action: "open_location", location: "farm_release" })}><Globe2 size={16} /> 공식 릴리스</Button>
        {update.target && <Button variant="primary" onPress={() => showUpdate(false)}><ArrowDownToLine size={16} /> 업데이트 보기</Button>}
      </div>
    </Panel>
  </div>;
}

export function UpdateDialog({ data, act, engine, close }: Props & { engine: boolean; close: () => void }) {
  const update = engine ? data.engine : data.update;
  const installing = update.installing;
  const queued = engine && data.engine.queued;
  const checking = engine ? data.engine.checking : data.update.busy;
  return <Dialog title={engine ? "EffectCraft 업데이트" : "Render Farm 업데이트"} onClose={close} locked={installing}
    description={engine ? "렌더에 참여하는 PC의 엔진 버전을 맞추세요." : "업데이트를 설치한 뒤 앱을 다시 시작합니다."}
    footer={<>
      {queued ? <Button variant="secondary" onPress={() => void act({ action: "cancel_engine_update" })}>설치 요청 취소</Button> :
        <Button variant="secondary" onPress={close} isDisabled={installing}>나중에</Button>}
      {!queued && !installing && <Button variant="primary" isDisabled={!data.windows || !update.target || checking || (engine ? data.update.busy : data.locked)}
        onPress={() => void act({ action: "install_update", engine })}>
        <ArrowDownToLine size={16} /> {data.locked && engine ? "연결 종료 후 설치" : engine ? "다운로드 및 설치" : "업데이트 및 재시작"}
      </Button>}
    </>}>
    <div className="version-comparison">
      <div><span>설치된 버전</span><strong>{engine ? data.engine.local ?? "—" : data.version}</strong></div>
      <ArrowUpRight size={20} />
      <div><span>업데이트할 버전</span><strong>{update.target ?? "—"}</strong></div>
    </div>
    {update.status && <p className="muted" role="status">{update.status}</p>}
    {engine && data.engine.peer && <p className="muted">상대 PC 버전 · {data.engine.peer}</p>}
    {queued && <Notice title="연결 종료를 기다리고 있습니다." />}
    {installing && <div className="form-loading update-transfer" role="status"><Spinner size="sm" /> {sizeMiB(update.bytes)} MiB 수신 · 설치를 준비하고 있습니다.</div>}
    {!engine && data.update.body && <div className="release-notes"><Separator /><p>{data.update.body}</p></div>}
    <Button variant="ghost" onPress={() => void act({ action: "open_location", location: engine ? "engine_release" : "farm_release" })}>공식 릴리스 보기 <ArrowUpRight size={16} /></Button>
  </Dialog>;
}
