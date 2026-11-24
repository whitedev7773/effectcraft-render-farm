import { Button, Card, Dropdown, Label, ProgressCircle, Separator, Surface, Tooltip } from "@heroui/react";
import type { ComponentProps, CSSProperties, ReactNode } from "react";
import type { Action, Page, Snapshot, Theme, WindowOperation } from "./api";
import { Graphic } from "./components";
import logoArt from "../../assets/ui/figma-welcome/icon-art.svg";
import home from "../../assets/ui/figma/topbar-home.svg";
import back from "../../assets/ui/figma/topbar-back.svg";
import moon from "../../assets/ui/figma/topbar-moon.svg";
import sun from "../../assets/ui/figma/topbar-sun.svg";
import system from "../../assets/ui/figma/theme-system.svg";
import update from "../../assets/ui/figma/topbar-update.svg";
import minimize from "../../assets/ui/figma/window-minimize.svg";
import maximize from "../../assets/ui/figma/window-expand.svg";
import restore from "../../assets/ui/figma/window-collapse.svg";
import close from "../../assets/ui/figma/window-close.svg";
import list from "../../assets/ui/figma/nav-list.svg";
import monitor from "../../assets/ui/figma/nav-monitor.svg";
import settings from "../../assets/ui/figma/nav-settings.svg";
import github from "../../assets/ui/figma-welcome/github.svg";
import server from "../../assets/ui/figma-welcome/server.svg";
import node from "../../assets/ui/figma-welcome/node.svg";
import applyUpdate from "../../assets/ui/figma-welcome/apply-update.svg";

function AppIcon({ size }: { size: 20 | 96 }) {
  return <span className="reference-logo" style={{ "--logo-size": `${size}px`, "--logo-scale": size / 512 } as CSSProperties}>
    <span className="reference-logo-layer"><Graphic src={logoArt} /></span>
  </span>;
}

type PictureButtonProps = Omit<ComponentProps<typeof Button>, "isIconOnly" | "children"> & { icon: string; label: string };
function PictureButton({ icon, label, ...props }: PictureButtonProps) {
  return <Tooltip delay={450}>
    <Button variant="ghost" isIconOnly aria-label={label} {...props}><Graphic src={icon} /></Button>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip>;
}

type ThemeProps = { theme: Theme; changeTheme: (theme: Theme) => void };
function ThemePicker({ theme, changeTheme }: ThemeProps) {
  const themes = [
    { id: "light", name: "라이트", icon: sun },
    { id: "dark", name: "다크", icon: moon },
    { id: "system", name: "시스템 설정", icon: system },
  ] as const;
  return <Dropdown>
    <Button variant="secondary" isIconOnly aria-label="화면 테마" className="reference-square theme-switch">
      <Graphic src={themes.find(({ id }) => id === theme)?.icon ?? moon} />
    </Button>
    <Dropdown.Popover>
      <Dropdown.Menu aria-label="화면 테마 선택" selectionMode="single" disallowEmptySelection selectedKeys={new Set([theme])}
        onSelectionChange={(selected) => {
          if (selected !== "all") {
            const key = [...selected][0];
            const target = themes.find(({ id }) => id === key);
            if (target) changeTheme(target.id);
          }
        }}>
        {themes.map(({ id, name, icon }) => <Dropdown.Item key={id} id={id} textValue={name}>
          <Graphic src={icon} /><Label>{name}</Label><Dropdown.ItemIndicator />
        </Dropdown.Item>)}
      </Dropdown.Menu>
    </Dropdown.Popover>
  </Dropdown>;
}

type WindowProps = { control: (operation: WindowOperation) => void; maximized: boolean };
function WindowButtons({ control, maximized, compact }: WindowProps & { compact?: boolean }) {
  return <div className="reference-window-buttons">
    <PictureButton icon={minimize} label="최소화" className="reference-square" onPress={() => control("minimize")} />
    {!compact && <PictureButton icon={maximized ? restore : maximize} label={maximized ? "이전 크기로 복원" : "최대화"}
      className="reference-square" onPress={() => control("maximize")} />}
    <PictureButton icon={close} label="닫기" className="reference-square reference-close" onPress={() => control("close")} />
  </div>;
}

function DragRegion({ control, className, canMaximize }: { control: WindowProps["control"]; className?: string; canMaximize?: boolean }) {
  return <div aria-hidden="true" className={className} onPointerDown={(event) => {
    if (event.button === 0) control("drag");
  }} onDoubleClick={() => { if (canMaximize) control("maximize"); }} />;
}

export function ResizeEdges({ control, maximized }: WindowProps) {
  if (maximized) return null;
  const edges = ["north", "south", "east", "west", "north_east", "north_west", "south_east", "south_west"] as const;
  return <>{edges.map((edge) => <div aria-hidden="true" key={edge} className={`window-edge edge-${edge}`} onPointerDown={(event) => {
    if (event.button === 0) control(edge);
  }} />)}</>;
}

export function Welcome({ data, act, chooseRole, showUpdate, changeTheme, control, maximized }: {
  data: Snapshot; act: Action; chooseRole: (host: boolean) => void; showUpdate: () => void;
} & WindowProps & Pick<ThemeProps, "changeTheme">) {
  return <div className="reference-welcome" data-maximized={maximized || undefined}>
    <DragRegion className="welcome-drag" control={control} />
    <div className="welcome-window-controls"><WindowButtons control={control} maximized={maximized} compact /></div>
    <header className="welcome-brand">
      <AppIcon size={96} />
      <h1>EffectCraft Render Farm</h1>
      <p>더 많은 PC로, 더 빠른 렌더링</p>
    </header>
    <div className="welcome-roles">
      <RoleCard host onPress={() => chooseRole(true)} />
      <RoleCard onPress={() => chooseRole(false)} />
    </div>
    <footer className="welcome-footer">
      <div className="welcome-footer-status">
        <ThemePicker theme={data.settings.theme} changeTheme={changeTheme} />
        {data.update.installing ? <ProgressCircle aria-label="업데이트 설치 중" isIndeterminate size="sm" className="welcome-update-progress">
          <ProgressCircle.Track><ProgressCircle.TrackCircle /><ProgressCircle.FillCircle /></ProgressCircle.Track>
          <Label>업데이트 설치 중</Label>
        </ProgressCircle> : data.update.target ? <Button variant="secondary" className="welcome-update" onPress={showUpdate}>
          <Graphic src={applyUpdate} /> 업데이트 적용
        </Button> : <span className="welcome-version">R.{data.version}</span>}
      </div>
      <Button variant="outline" className="welcome-source" onPress={() => void act({ action: "open_location", location: "engine_source" })}>
        <Graphic src={github} /> EffectCraft
      </Button>
    </footer>
    <ResizeEdges control={control} maximized={maximized} />
  </div>;
}

function RoleCard({ host = false, onPress }: { host?: boolean; onPress: () => void }) {
  return <Card className="welcome-role-card">
    <Button variant="ghost" onPress={onPress} className="welcome-role-button">
      <Graphic src={host ? server : node} />
      <strong>{host ? "서버로 실행" : "노드로 실행"}</strong>
      <span>{host ? <>프로젝트의 렌더링 작업을 관리하고<br />연결된 PC에 맞춰 렌더링 범위를 분배해요</>
        : <>이 PC의 자원을 활용하여 다른 기기의<br />렌더링 작업을 함께 처리해요.</>}</span>
    </Button>
  </Card>;
}

export function WorkspaceShell({ data, page, navigate, homeAction, showUpdate, focus, children, changeTheme, control, maximized }: {
  data: Snapshot; page: Page; navigate: (page: Page) => void; homeAction: () => void; showUpdate: () => void;
  focus: React.Ref<HTMLElement>; children: ReactNode;
} & WindowProps & Pick<ThemeProps, "changeTheme">) {
  const tabs = [
    { id: data.settings.host ? "queue" : "nodes", label: data.settings.host ? "목록" : "연결", image: list },
    { id: "monitor", label: "모니터", image: monitor },
    { id: "settings", label: "설정", image: settings },
  ] as const;
  const hasUpdate = Boolean(data.engine.target || data.update.target || data.update.installing || data.engine.installing);
  const active = page === "queue_tools" ? "queue" : page === "logs" || (page === "nodes" && data.settings.host) ? "settings" : page;
  return <div className="reference-workspace" data-maximized={maximized || undefined}>
    <header className="reference-topbar">
      <DragRegion control={control} canMaximize className="workspace-drag" />
      <div className="reference-top-left">
        <Button variant="outline" className="reference-home" onPress={homeAction} aria-label="시작 화면" isDisabled={data.locked || Boolean(data.import)}>
          <Graphic src={back} /><Graphic src={home} />
        </Button>
        <div className="reference-brand"><AppIcon size={20} /><span>EffectCraft Render Farm</span><span className="reference-version">R.{data.version}</span></div>
      </div>
      <div className="reference-top-end">
        <PictureButton variant="primary" className="reference-square"
          icon={update} label={hasUpdate ? "업데이트 보기" : "업데이트 상태"} onPress={showUpdate} />
        <ThemePicker theme={data.settings.theme} changeTheme={changeTheme} />
        <WindowButtons control={control} maximized={maximized} />
      </div>
    </header>
    <div className="reference-body">
      <Surface className="reference-navigation">
        <nav aria-label="작업 공간 메뉴">
          {tabs.map(({ id, label, image }) => <div key={id} className="reference-nav-item">
            {id === "settings" && <Separator />}
            <Button variant="ghost" data-active={active === id || undefined} aria-current={active === id ? "page" : undefined} onPress={() => navigate(id)}>
              <Graphic src={image} />{label}
            </Button>
          </div>)}
        </nav>
      </Surface>
      <main ref={focus} tabIndex={-1} id="workspace" className="reference-main">{children}</main>
    </div>
    <ResizeEdges control={control} maximized={maximized} />
  </div>;
}
