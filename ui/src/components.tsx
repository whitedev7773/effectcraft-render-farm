import {
  Alert, Button, Card, Chip, EmptyState, Input, Label, ListBox, Modal, NumberField, ProgressBar, SearchField,
  Select, Switch, Table, TextField, Tooltip,
} from "@heroui/react";
import { useEffect, useEffectEvent, useRef, useState, type ComponentProps, type ReactNode, type Ref } from "react";
import { Minus, Plus, type LucideIcon } from "lucide-react";
import { count, type Preview, type Task } from "./api";
import emptyFrame from "../../assets/ui/figma/status-empty-broadcast.svg";
import closeIcon from "../../assets/ui/figma/window-close.svg";

export function Graphic({ src, className = "" }: { src: string; className?: string }) {
  return <img src={src} alt="" draggable={false} className={`figma-graphic ${className}`} />;
}

export function PageHeading({ title, description, actions, className = "", actionsClassName = "" }: {
  title: string; description?: ReactNode; actions?: ReactNode; className?: string; actionsClassName?: string;
}) {
  return <header className={`page-heading ${className}`}>
    <div><h1>{title}</h1>{description && <p>{description}</p>}</div>
    {actions && <div className={`page-actions ${actionsClassName}`}>{actions}</div>}
  </header>;
}

export function IconButton({ label, icon: Icon, ...props }: Omit<ComponentProps<typeof Button>, "children" | "aria-label" | "isIconOnly"> & {
  label: string; icon: LucideIcon;
}) {
  return <Tooltip delay={450}>
    <Button isIconOnly aria-label={label} variant="ghost" size="sm" {...props}><Icon size={17} aria-hidden="true" /></Button>
    <Tooltip.Content>{label}</Tooltip.Content>
  </Tooltip>;
}

export function Panel({ title, description, icon: Icon, action, footer, children, className = "" }: {
  title: string; description?: string; icon?: LucideIcon | string; action?: ReactNode;
  footer?: ReactNode; children: ReactNode; className?: string;
}) {
  return <Card className={`panel ${className}`}>
    <Card.Header className="panel-header">
      <div className="panel-title-group">
        {Icon && <span className="panel-icon">{typeof Icon === "string" ? <Graphic src={Icon} /> : <Icon size={16} aria-hidden="true" />}</span>}
        <div><Card.Title>{title}</Card.Title>{description && <Card.Description>{description}</Card.Description>}</div>
      </div>
      {action}
    </Card.Header>
    <Card.Content className="panel-content">{children}</Card.Content>
    {footer && <Card.Footer className="panel-footer">{footer}</Card.Footer>}
  </Card>;
}

export function Empty({ title, icon, children, muted, className = "" }: {
  title: string; icon?: string; children?: ReactNode; muted?: boolean; className?: string;
}) {
  return <EmptyState className={`reference-empty ${className}`} data-muted={muted || undefined}>
    {icon && <Graphic src={icon} />}
    <div><strong>{title}</strong>{children && <p>{children}</p>}</div>
  </EmptyState>;
}

export function DataTable({ label, children, className = "" }: { label: string; children: ReactNode; className?: string }) {
  return <Table className={`farm-table ${className}`}>
    <Table.ScrollContainer>
      <Table.Content aria-label={label}>{children}</Table.Content>
    </Table.ScrollContainer>
  </Table>;
}

export function StatusChip({ task }: { task: Task }) {
  const color = task.status === "Completed" ? "success" : task.status === "Failed" ? "danger"
    : task.status === "Interrupted" || task.status === "Waiting" ? "warning"
    : task.status === "Pending" ? "default" : "accent";
  return <Chip size="sm" variant="soft" color={color}><span className="chip-dot" aria-hidden="true" /><Chip.Label>{task.status_label}</Chip.Label></Chip>;
}

export function Progress({ label, value }: { label: string; value: number }) {
  return <ProgressBar aria-label={label} value={value} className="farm-progress">
    <ProgressBar.Track><ProgressBar.Fill /></ProgressBar.Track>
  </ProgressBar>;
}

export function Search({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return <SearchField aria-label={label} value={value} onChange={onChange} className="farm-search">
    <SearchField.Group>
      <SearchField.SearchIcon />
      <SearchField.Input placeholder={label} />
      <SearchField.ClearButton aria-label="검색어 지우기" />
    </SearchField.Group>
  </SearchField>;
}

export function Choice({ label, value, options, onChange, disabled, compact = false, triggerRef }: {
  label: string; value: string | null; options: { value: string; label: string }[];
  onChange: (value: string) => void; disabled?: boolean; compact?: boolean; triggerRef?: Ref<HTMLButtonElement>;
}) {
  return <Select aria-label={label} value={value} onChange={(key) => { if (key !== null) onChange(String(key)); }}
    isDisabled={disabled} placeholder="선택하세요" className={compact ? "compact-choice" : "farm-field"}>
    {!compact && <Label>{label}</Label>}
    <Select.Trigger ref={triggerRef}><Select.Value /><Select.Indicator /></Select.Trigger>
    <Select.Popover>
      <ListBox>{options.map((option) => <ListBox.Item key={option.value} id={option.value} textValue={option.label}>
        {option.label}<ListBox.ItemIndicator />
      </ListBox.Item>)}</ListBox>
    </Select.Popover>
  </Select>;
}

export function Field({ label, value, onChange, disabled, placeholder, secret, extra }: {
  label: string; value: string; onChange: (value: string) => void; disabled?: boolean; placeholder?: string; secret?: boolean; extra?: ReactNode;
}) {
  return <TextField className="farm-field" value={value} onChange={onChange} isDisabled={disabled}>
    <Label>{label}</Label>
    <div className="field-inline"><Input placeholder={placeholder} type={secret ? "password" : "text"} autoComplete="off" spellCheck={false} />{extra}</div>
  </TextField>;
}

export function Numeric({ label, value, onChange, min, max, step = 1, disabled }: {
  label: string; value: number; onChange: (value: number) => void; min: number; max: number; step?: number; disabled?: boolean;
}) {
  return <NumberField className="farm-field" value={value} onChange={(next) => { if (Number.isFinite(next)) onChange(next); }}
    minValue={min} maxValue={max} step={step} isDisabled={disabled} formatOptions={{ useGrouping: false, maximumFractionDigits: 2 }}>
    <Label>{label}</Label>
    <NumberField.Group>
      <NumberField.DecrementButton aria-label={`${label} 줄이기`}><Minus size={15} /></NumberField.DecrementButton>
      <NumberField.Input />
      <NumberField.IncrementButton aria-label={`${label} 늘리기`}><Plus size={15} /></NumberField.IncrementButton>
    </NumberField.Group>
  </NumberField>;
}

export function Toggle({ label, description, enabled, onChange, disabled }: {
  label: string; description?: string; enabled: boolean; onChange: (enabled: boolean) => void; disabled?: boolean;
}) {
  return <Switch className="toggle-row" isSelected={enabled} onChange={onChange} isDisabled={disabled} aria-label={label}>
    <div><Label>{label}</Label>{description && <p>{description}</p>}</div>
    <Switch.Content><Switch.Control><Switch.Thumb /></Switch.Control></Switch.Content>
  </Switch>;
}

export function Notice({ title, children, danger = false }: { title: string; children?: ReactNode; danger?: boolean }) {
  return <Alert status={danger ? "danger" : "warning"} className="farm-alert">
    <Alert.Indicator /><Alert.Content><Alert.Title>{title}</Alert.Title>{children && <Alert.Description>{children}</Alert.Description>}</Alert.Content>
  </Alert>;
}

export function Dialog({ title, description, children, footer, onClose, wide, locked }: {
  title: string; description?: string; children: ReactNode; footer?: ReactNode | ((dismiss: () => void) => ReactNode);
  onClose: () => void | Promise<void>; wide?: boolean; locked?: boolean;
}) {
  const [dismissing, setDismissing] = useState(false);
  const backdrop = useRef<HTMLDivElement>(null);
  const dismiss = () => { if (!locked) setDismissing(true); };
  const finishDismissal = useEffectEvent(async () => {
    try { if (!locked) await onClose(); }
    finally { setDismissing(false); }
  });
  useEffect(() => {
    if (!dismissing) return;
    let mounted = true;
    const animations = backdrop.current?.getAnimations() ?? [];
    void Promise.allSettled(animations.map((animation) => animation.finished)).then(() => {
      if (mounted) void finishDismissal();
    });
    return () => { mounted = false; };
  }, [dismissing]);

  return <Modal>
    <Modal.Backdrop ref={backdrop} isOpen isDismissable={!locked && !dismissing} isKeyboardDismissDisabled={locked || dismissing}
      data-dismissing={dismissing || undefined}
      onOpenChange={(open) => { if (!open) dismiss(); }}>
      <Modal.Container placement="center" size={wide ? "lg" : "md"} scroll="inside">
        <Modal.Dialog className="farm-dialog">
          <Modal.Header>
            <Modal.Heading>{title}</Modal.Heading>
            {description && <p className="muted">{description}</p>}
            {!locked && <Modal.CloseTrigger aria-label="닫기" className="farm-dialog-close"><Graphic src={closeIcon} /></Modal.CloseTrigger>}
          </Modal.Header>
          <Modal.Body>{children}</Modal.Body>
          {footer && <Modal.Footer>{typeof footer === "function" ? footer(dismiss) : footer}</Modal.Footer>}
        </Modal.Dialog>
      </Modal.Container>
    </Modal.Backdrop>
  </Modal>;
}

export function FrameImage({ preview, empty = "아직 표시할 프레임이 없습니다." }: { preview: Preview | null; empty?: string }) {
  return <figure className="frame-preview">
    {preview ? <PreviewImage key={preview.url} preview={preview} /> :
      <Empty title={empty} icon={emptyFrame} className="frame-placeholder" />}
  </figure>;
}

function PreviewImage({ preview }: { preview: Preview }) {
  const [failed, setFailed] = useState(false);
  return failed ? <Empty title="미리보기를 불러오지 못했습니다" icon={emptyFrame} className="frame-placeholder" /> : <>
    <img className="frame-bitmap" src={preview.url} alt={`렌더 프레임 ${count(preview.frame)}`} onError={() => setFailed(true)} />
    <figcaption>프레임 {count(preview.frame)}</figcaption>
  </>;
}
