use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::thread;
use std::time::Duration;

use effectcraft_keyframe::Value;
use effectcraft_project::props::{Node, PropGroup};
use effectcraft_project::render_queue::sequence_path;
use effectcraft_project::{Footage, FootageKind, ItemId, ItemKind, LayerSource, Project};
use effectcraft_time::{FrameRate, Tick};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

fn project_hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

const MAX_LINE: usize = 4096;
const MAX_FRAMES: i64 = 100_000;
const MAX_WORKERS: usize = 64;
const PROTOCOL_VERSION: u32 = 4;
const MAX_TRANSFER_FILE: u64 = 32 * 1024 * 1024 * 1024;
const MAX_TRANSFER_TOTAL: u64 = 256 * 1024 * 1024 * 1024;
const MAX_TRANSFER_FILES: usize = 20_000;
const PNG_HEADER: &[u8] = b"\x89PNG\r\n\x1a\n";

fn comp_frame_count(rate: FrameRate, duration: Tick) -> Result<i64, String> {
    if rate.num <= 0 || rate.den <= 0 || !rate.as_f64().is_finite() {
        return Err("잘못된 컴포지션 프레임 레이트입니다".into());
    }
    let first_after = rate.frame_at(duration);
    let total =
        if rate.tick_of(first_after) < duration { first_after.checked_add(1).ok_or("컴포지션 프레임 수가 너무 큽니다")? } else { first_after };
    if !(1..=MAX_FRAMES).contains(&total) {
        return Err(format!("컴포지션 길이는 1~{MAX_FRAMES}프레임이어야 합니다 (현재 {total}프레임)"));
    }
    Ok(total)
}

fn ceil_frame(rate: FrameRate, tick: Tick) -> i64 {
    let frame = rate.frame_at(tick);
    if rate.tick_of(frame) < tick { frame.saturating_add(1) } else { frame }
}

fn work_frame_range(rate: FrameRate, duration: Tick, work_area: (Tick, Tick), total: i64) -> (i64, i64) {
    let start = work_area.0.max(Tick::ZERO).min(duration);
    let end = work_area.1.max(Tick::ZERO).min(duration);
    if end <= start {
        return (0, total);
    }
    let first = ceil_frame(rate, start).clamp(0, total);
    let last = ceil_frame(rate, end).clamp(0, total);
    if first >= last { (0, total) } else { (first, last) }
}

#[derive(Clone)]
pub struct Config {
    pub project: PathBuf,
    pub cli: PathBuf,
    pub output: PathBuf,
    pub comp: String,
    pub address: String,
    pub token: String,
    pub gpu: bool,
    pub range_start: i64,
    pub range_end: i64,
    pub chunk_frames: i64,
    pub max_workers: usize,
    pub final_video: PathBuf,
    pub disabled_workers: Vec<String>,
}

#[derive(Clone)]
pub struct Job {
    pub hash: String,
    pub comp: String,
    pub rate: FrameRate,
    pub total: i64,
    pub start: i64,
    pub end: i64,
    pub width: u32,
    pub height: u32,
    pub output: PathBuf,
}

#[derive(Clone)]
pub struct CompInfo {
    pub name: String,
    pub frames: i64,
    pub rate: FrameRate,
    pub width: u32,
    pub height: u32,
    pub work_start: i64,
    pub work_end: i64,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Manifest {
    hash: String,
    comp: String,
    fps_num: i64,
    fps_den: i64,
    frames: i64,
    width: u32,
    height: u32,
    #[serde(default)]
    start: i64,
    #[serde(default)]
    end: i64,
}

impl From<&Job> for Manifest {
    fn from(job: &Job) -> Self {
        Self {
            hash: job.hash.clone(),
            comp: job.comp.clone(),
            fps_num: job.rate.num,
            fps_den: job.rate.den,
            frames: job.total,
            width: job.width,
            height: job.height,
            start: job.start,
            end: job.end,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChunkStatus {
    Pending,
    Running,
    Done,
    Failed,
}

struct Chunk {
    start: i64,
    end: i64,
    status: ChunkStatus,
    attempts: u8,
    worker: String,
}

#[derive(Clone)]
pub struct WorkerInfo {
    pub id: u64,
    pub ip: String,
    pub label: String,
    pub enabled: bool,
    pub connected: bool,
    pub assigned: Option<(i64, i64)>,
    pub current_done: i64,
    pub completed_frames: i64,
}

#[derive(Default)]
pub struct State {
    chunks: Vec<Chunk>,
    pub messages: Vec<String>,
    pub workers: usize,
    pub worker_info: BTreeMap<u64, WorkerInfo>,
    next_worker_id: u64,
    pub max_workers: usize,
    pub dispatch_enabled: bool,
    pub assembly_started: bool,
    pub final_status: String,
    disabled_workers: Vec<String>,
    pub total: i64,
    pub output: PathBuf,
    bundle: Arc<OnceLock<Result<Arc<TransferBundle>, String>>>,
    pub local_progress: Option<(i64, i64, i64)>,
    pub log_path: Option<PathBuf>,
    log_error_reported: bool,
}

pub type Shared = Arc<Mutex<State>>;

fn append_log(path: &Path, message: &str) -> Result<(), String> {
    let parent = path.parent().ok_or("로그 폴더가 없습니다")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("로그 폴더 만들기: {e}"))?;
    if std::fs::metadata(path).is_ok_and(|metadata| metadata.len() > 5_000_000) {
        let previous = path.with_extension("old.log");
        if previous.exists() {
            std::fs::remove_file(&previous).map_err(|e| format!("이전 로그 정리: {e}"))?;
        }
        std::fs::rename(path, previous).map_err(|e| format!("로그 교체: {e}"))?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|e| format!("로그 열기: {e}"))?;
    let seconds = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    writeln!(file, "[{seconds}] {message}").map_err(|e| format!("로그 쓰기: {e}"))
}

impl State {
    pub fn with_log_path(path: Option<PathBuf>) -> Self {
        Self { log_path: path, ..Self::default() }
    }

    pub fn note(&mut self, message: impl Into<String>) {
        let message = message.into();
        if let Some(path) = &self.log_path
            && let Err(error) = append_log(path, &message)
            && !self.log_error_reported
        {
            self.log_error_reported = true;
            self.messages.push(format!("로그 파일 저장 실패: {error}"));
        }
        self.messages.push(message);
        if self.messages.len() > 100 {
            self.messages.remove(0);
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Message {
    Hello {
        token: String,
        name: String,
        #[serde(default)]
        protocol: u32,
    },
    Ready {
        comp: String,
    },
    Bundle {
        comp: String,
        project_len: u64,
        project_sha256: String,
        file_count: usize,
        total: i64,
        fps_num: i64,
        fps_den: i64,
        width: u32,
        height: u32,
    },
    File {
        name: String,
        len: u64,
        sha256: String,
    },
    Frame {
        frame: i64,
        len: u64,
        sha256: String,
    },
    Progress {
        start: i64,
        end: i64,
        done: i64,
    },
    Request,
    Assign {
        start: i64,
        end: i64,
        fps_num: i64,
        fps_den: i64,
        comp: String,
    },
    Complete {
        start: i64,
        end: i64,
        ok: bool,
        detail: String,
    },
    Disconnect,
    Wait,
    Finished,
    Error {
        detail: String,
    },
}

fn lock(state: &Shared) -> std::sync::MutexGuard<'_, State> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

fn log(state: &Shared, message: impl Into<String>) {
    let mut state = lock(state);
    state.note(message);
}

pub fn read_job(config: &Config) -> Result<Job, String> {
    read_job_impl(config, true)
}

fn read_job_without_preflight(config: &Config) -> Result<Job, String> {
    read_job_impl(config, false)
}

fn read_job_impl(config: &Config, preflight: bool) -> Result<Job, String> {
    if config.token.trim().len() < 8 {
        return Err("접속 암호는 8자 이상이어야 합니다".into());
    }
    if !config.cli.is_file() {
        return Err(format!("렌더 CLI 파일이 없습니다: {}", config.cli.display()));
    }
    let bytes = read_project_bytes(&config.project)?;
    let source = std::str::from_utf8(&bytes).map_err(|e| format!("프로젝트 문자 인코딩: {e}"))?;
    let project = Project::from_json(source).map_err(|e| format!("프로젝트 열기: {e}"))?;
    let matches: Vec<_> = project.comps().filter(|(id, _)| project.item(**id).is_some_and(|item| item.name == config.comp)).collect();
    let [(comp_id, comp)] = matches.as_slice() else {
        return Err(format!("컴포지션 이름이 없거나 중복됩니다: {}", config.comp));
    };
    let rate = comp.frame_rate;
    let total = comp_frame_count(rate, comp.duration)?;
    if !config.output.is_dir() {
        return Err(format!("프레임 저장 폴더가 없습니다: {}", config.output.display()));
    }
    let start = config.range_start;
    let end = if config.range_end == 0 { total } else { config.range_end };
    if start < 0 || end <= start || end > total {
        return Err(format!("렌더 범위는 0 이상 {total} 이하이며 시작 < 끝이어야 합니다"));
    }
    if preflight {
        preflight_resources(&project, **comp_id, &config.project)?;
    }
    Ok(Job {
        hash: project_hash(&bytes),
        comp: config.comp.clone(),
        rate,
        total,
        start,
        end,
        width: comp.width,
        height: comp.height,
        output: config.output.clone(),
    })
}

pub fn list_compositions(path: &Path) -> Result<Vec<CompInfo>, String> {
    let bytes = read_project_bytes(path)?;
    let source = std::str::from_utf8(&bytes).map_err(|e| format!("프로젝트 문자 인코딩: {e}"))?;
    let project = Project::from_json(source).map_err(|e| format!("프로젝트 열기: {e}"))?;
    let mut comps = project
        .comps()
        .filter_map(|(id, comp)| {
            let name = project.item(*id)?.name.clone();
            let rate = comp.frame_rate;
            let frames = comp_frame_count(rate, comp.duration).unwrap_or(0);
            let (work_start, work_end) = if frames > 0 { work_frame_range(rate, comp.duration, comp.work_area, frames) } else { (0, 0) };
            Some(CompInfo { name, frames, rate, width: comp.width, height: comp.height, work_start, work_end })
        })
        .collect::<Vec<_>>();
    comps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(comps)
}

fn read_project_bytes(path: &Path) -> Result<Vec<u8>, String> {
    if std::fs::metadata(path).map_err(|e| format!("프로젝트 정보 읽기: {e}"))?.len() > 100_000_000 {
        return Err("프로젝트가 100MB 제한을 넘습니다".into());
    }
    let bytes = std::fs::read(path).map_err(|e| format!("프로젝트 읽기: {e}"))?;
    if bytes.len() > 100_000_000 {
        return Err("프로젝트가 100MB 제한을 넘습니다".into());
    }
    Ok(bytes)
}

/// Check files and fonts used by the selected comp before any frame is assigned.
fn preflight_resources(project: &Project, root: ItemId, project_path: &Path) -> Result<(), String> {
    let project_path = std::path::absolute(project_path).map_err(|e| format!("프로젝트 경로 확인: {e}"))?;
    let mut seen = BTreeSet::new();
    let mut pending = vec![(root, 0_usize)];
    let mut missing = BTreeSet::new();
    while let Some((id, depth)) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if depth > 64 {
            return Err("컴포지션 중첩이 64단계를 넘습니다".into());
        }
        let Some(item) = project.item(id) else {
            return Err(format!("프로젝트의 참조 항목을 찾을 수 없습니다: {}", id.0));
        };
        match &item.kind {
            ItemKind::Comp(comp) => {
                if let Some(proxy) = item.proxy.as_ref().filter(|p| p.enabled) {
                    check_footage(&proxy.footage, &project_path, &mut missing);
                }
                for layer in &comp.layers {
                    collect_fonts(&layer.props, &mut missing)?;
                    if let Some(source) = layer.source.item() {
                        pending.push((source, depth + 1));
                    }
                }
            }
            ItemKind::Footage(footage) => {
                check_footage(footage, &project_path, &mut missing);
                if let Some(proxy) = item.proxy.as_ref().filter(|p| p.enabled) {
                    check_footage(&proxy.footage, &project_path, &mut missing);
                }
            }
            ItemKind::Folder | ItemKind::Solid(_) => {}
        }
        if missing.len() > 1000 {
            return Err("누락된 미디어 또는 글꼴이 1,000개를 넘습니다".into());
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        let details = missing.iter().take(20).cloned().collect::<Vec<_>>().join("\n");
        let more = if missing.len() > 20 { format!("\n외 {}개", missing.len() - 20) } else { String::new() };
        Err(format!("이 PC에서 렌더에 필요한 리소스 {}개를 찾지 못했습니다:\n{details}{more}", missing.len()))
    }
}

fn check_footage(footage: &Footage, project_path: &Path, missing: &mut BTreeSet<String>) {
    if footage.kind == FootageKind::Data && footage.data.is_some() {
        return;
    }
    if footage.path.is_empty() && footage.sequence.is_empty() && footage.data.is_none() {
        missing.insert("미디어 경로가 비어 있음".into());
    }
    for stored in std::iter::once(&footage.path).chain(&footage.sequence).filter(|path| !path.is_empty()) {
        let document = project_path.to_string_lossy();
        match craft_docpath::resolve(stored, Some(Path::new(document.as_ref()))) {
            Ok(path) if path.is_file() => {}
            Ok(path) => {
                missing.insert(format!("미디어: {} ({})", stored, path.display()));
            }
            Err(reason) => {
                missing.insert(format!("미디어 경로: {stored} ({reason})"));
            }
        }
    }
}

fn collect_fonts(root: &PropGroup, missing: &mut BTreeSet<String>) -> Result<(), String> {
    let mut pending = vec![(root, 0_usize)];
    while let Some((group, depth)) = pending.pop() {
        if depth > 64 {
            return Err("속성 그룹 중첩이 64단계를 넘습니다".into());
        }
        for node in &group.children {
            match node {
                Node::Group(child) => pending.push((child, depth + 1)),
                Node::Prop(prop) => {
                    for value in std::iter::once(&prop.value).chain(prop.keys.iter().map(|key| &key.value)) {
                        if let Value::Text(doc) = value {
                            check_font(&doc.font, &doc.style, missing);
                            for run in &doc.runs {
                                check_font(&run.style.font, &run.style.style, missing);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn check_font(family: &str, style: &str, missing: &mut BTreeSet<String>) {
    if effectcraft_text::fonts::resolve(family, style).missing {
        missing.insert(format!("글꼴: {family} / {style}"));
    }
}

struct TransferFile {
    name: String,
    source: PathBuf,
    len: u64,
    sha256: String,
}

struct TransferBundle {
    project: Vec<u8>,
    project_sha256: String,
    files: Vec<TransferFile>,
}

#[derive(Default)]
struct BundleBuilder {
    files: Vec<TransferFile>,
    names: BTreeMap<PathBuf, String>,
    bytes: u64,
}

fn file_sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("파일 열기 {}: {e}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buf = [0_u8; 128 * 1024];
    loop {
        let n = file.read(&mut buf).map_err(|e| format!("파일 읽기 {}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

impl BundleBuilder {
    fn add(&mut self, source: PathBuf, folder: &str) -> Result<String, String> {
        let source = std::fs::canonicalize(&source).map_err(|e| format!("소스 경로 {}: {e}", source.display()))?;
        if let Some(name) = self.names.get(&source) {
            return Ok(name.clone());
        }
        if self.files.len() >= MAX_TRANSFER_FILES {
            return Err("전송할 파일이 20,000개를 넘습니다".into());
        }
        let len = std::fs::metadata(&source).map_err(|e| format!("소스 정보 {}: {e}", source.display()))?.len();
        self.bytes = self.bytes.checked_add(len).ok_or("전송 크기가 너무 큽니다")?;
        if len > MAX_TRANSFER_FILE || self.bytes > MAX_TRANSFER_TOTAL {
            return Err("소스 파일 또는 전송 합계가 크기 제한을 넘습니다".into());
        }
        let extension = source
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| !e.is_empty() && e.len() <= 12 && e.bytes().all(|c| c.is_ascii_alphanumeric()))
            .unwrap_or("bin");
        let name = format!("{folder}/asset_{:06}.{extension}", self.files.len());
        let sha256 = file_sha256(&source)?;
        self.files.push(TransferFile { name: name.clone(), source: source.clone(), len, sha256 });
        self.names.insert(source, name.clone());
        Ok(name)
    }
}

fn selected_items(project: &Project, root: ItemId) -> Result<Vec<ItemId>, String> {
    let mut seen = BTreeSet::new();
    let mut pending = vec![(root, 0_usize)];
    while let Some((id, depth)) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        if depth > 64 || seen.len() > MAX_TRANSFER_FILES {
            return Err("프로젝트 참조가 너무 깊거나 많습니다".into());
        }
        let item = project.item(id).ok_or_else(|| format!("참조 항목 {}가 없습니다", id.0))?;
        if let ItemKind::Comp(comp) = &item.kind {
            for layer in &comp.layers {
                if let Some(source) = layer.source.item() {
                    pending.push((source, depth + 1));
                }
            }
        }
    }
    Ok(seen.into_iter().collect())
}

fn rewrite_footage(footage: &mut Footage, project_path: &Path, builder: &mut BundleBuilder) -> Result<(), String> {
    if footage.kind == FootageKind::Data && footage.data.is_some() {
        return Ok(());
    }
    for stored in std::iter::once(&mut footage.path).chain(footage.sequence.iter_mut()) {
        if stored.is_empty() {
            continue;
        }
        let source = craft_docpath::resolve(stored, Some(project_path)).map_err(|e| format!("미디어 경로 {stored}: {e}"))?;
        *stored = builder.add(source, "media")?;
    }
    Ok(())
}

fn add_fonts(root: &PropGroup, builder: &mut BundleBuilder) -> Result<(), String> {
    let mut pending = vec![(root, 0_usize)];
    while let Some((group, depth)) = pending.pop() {
        if depth > 64 {
            return Err("속성 그룹 중첩이 너무 깊습니다".into());
        }
        for node in &group.children {
            match node {
                Node::Group(child) => pending.push((child, depth + 1)),
                Node::Prop(prop) => {
                    for value in std::iter::once(&prop.value).chain(prop.keys.iter().map(|key| &key.value)) {
                        if let Value::Text(doc) = value {
                            for (family, style) in
                                std::iter::once((&doc.font, &doc.style)).chain(doc.runs.iter().map(|run| (&run.style.font, &run.style.style)))
                            {
                                let resolved = effectcraft_text::fonts::resolve(family, style);
                                if resolved.missing {
                                    return Err(format!("호스트 글꼴을 찾을 수 없습니다: {family} / {style}"));
                                }
                                if let effectcraft_text::fonts::FaceData::File(path) = &effectcraft_text::fonts::face(resolved.face).info.data {
                                    builder.add(path.clone(), "fonts")?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

fn build_bundle(config: &Config, job: &Job) -> Result<TransferBundle, String> {
    let bytes = read_project_bytes(&config.project)?;
    if project_hash(&bytes) != job.hash {
        return Err("렌더링 중 원본 프로젝트 파일이 변경됐습니다".into());
    }
    let source = std::str::from_utf8(&bytes).map_err(|e| format!("프로젝트 문자 인코딩: {e}"))?;
    let mut project = Project::from_json(source).map_err(|e| format!("프로젝트 열기: {e}"))?;
    let root = project.comps().find_map(|(id, _)| project.item(*id).filter(|item| item.name == job.comp).map(|_| *id)).ok_or("컴포지션을 찾지 못했습니다")?;
    let ids = selected_items(&project, root)?;
    let project_path = std::fs::canonicalize(&config.project).map_err(|e| format!("프로젝트 경로 확인: {e}"))?;
    let mut builder = BundleBuilder::default();
    for id in ids {
        let item = project.item_mut(id).ok_or("참조 항목이 사라졌습니다")?;
        if let ItemKind::Comp(comp) = &item.kind {
            for layer in &comp.layers {
                add_fonts(&layer.props, &mut builder)?;
            }
        }
        if let ItemKind::Footage(footage) = &mut item.kind {
            rewrite_footage(footage, &project_path, &mut builder)?;
        }
        if let Some(proxy) = item.proxy.as_mut().filter(|proxy| proxy.enabled) {
            rewrite_footage(&mut proxy.footage, &project_path, &mut builder)?;
        }
    }
    let project = project.to_file_json().map_err(|e| format!("전송용 프로젝트 생성: {e}"))?.into_bytes();
    if project.len() > 100_000_000 {
        return Err("전송용 프로젝트가 100MB를 넘습니다".into());
    }
    Ok(TransferBundle { project_sha256: project_hash(&project), project, files: builder.files })
}

fn prepare_output(job: &Job) -> Result<(), String> {
    let path = job.output.join("effectcraft-farm.json");
    let expected = Manifest::from(job);
    if path.exists() {
        if std::fs::metadata(&path).map_err(|e| format!("작업 정보 확인: {e}"))?.len() > 128_000 {
            return Err("작업 정보 파일이 너무 큽니다".into());
        }
        let bytes = std::fs::read(&path).map_err(|e| format!("작업 정보 읽기: {e}"))?;
        let mut existing: Manifest = serde_json::from_slice(&bytes).map_err(|e| format!("작업 정보 파일 오류: {e}"))?;
        if existing.end == 0 {
            existing.end = existing.frames;
        }
        if existing != expected {
            return Err("프레임 저장 폴더에 다른 프로젝트의 렌더 결과가 있습니다".into());
        }
    } else {
        let mut has_frames = false;
        for entry in std::fs::read_dir(&job.output).map_err(|e| format!("출력 폴더 읽기: {e}"))? {
            let entry = entry.map_err(|e| format!("출력 폴더 항목 읽기: {e}"))?;
            if entry.file_name().to_string_lossy().starts_with("frame_") {
                has_frames = true;
                break;
            }
        }
        if has_frames {
            return Err("프레임 저장 폴더에 작업 정보 없는 기존 frame_ 파일이 있습니다".into());
        }
        let bytes = serde_json::to_vec_pretty(&expected).map_err(|e| e.to_string())?;
        std::fs::write(&path, bytes).map_err(|e| format!("작업 정보 저장: {e}"))?;
    }
    Ok(())
}

fn frame_path(output: &Path, frame: i64) -> PathBuf {
    PathBuf::from(sequence_path(&output.join("frame_[#####].png").to_string_lossy(), frame))
}

fn valid_png(path: &Path) -> bool {
    let Ok(mut file) = std::fs::File::open(path) else { return false };
    let mut header = [0u8; 8];
    if file.read_exact(&mut header).is_err() || header != PNG_HEADER {
        return false;
    }
    if file.seek(SeekFrom::End(-12)).is_err() {
        return false;
    }
    let mut trailer = [0u8; 12];
    file.read_exact(&mut trailer).is_ok() && trailer.get(4..8) == Some(b"IEND".as_slice())
}

fn chunk_complete(output: &Path, start: i64, end: i64) -> bool {
    (start..end).all(|frame| valid_png(&frame_path(output, frame)))
}

fn connect_to_host(address: &str) -> Result<TcpStream, String> {
    let target = address
        .trim()
        .parse::<SocketAddr>()
        .map_err(|_| format!("호스트 주소 '{address}'가 올바르지 않습니다. 호스트 PC의 고정 LAN IP와 포트를 입력하세요 (예: 192.168.0.10:9778)"))?;
    TcpStream::connect_timeout(&target, Duration::from_secs(6)).map_err(|error| {
        let reason = match error.kind() {
            std::io::ErrorKind::ConnectionRefused => "호스트가 이 IP:포트에서 대기하지 않습니다. 호스트 앱에서 '호스트 열기'가 완료됐는지 확인하세요.",
            std::io::ErrorKind::TimedOut => "연결 시간이 초과됐습니다. 두 PC의 LAN 연결, 호스트 IP, Windows 방화벽의 TCP 9778 허용을 확인하세요.",
            std::io::ErrorKind::AddrNotAvailable => "이 PC에서 해당 주소로 연결할 수 없습니다. 호스트 PC의 LAN IP를 확인하세요.",
            _ => "호스트 IP와 포트, LAN 연결, 방화벽 설정을 확인하세요.",
        };
        let loopback = if target.ip().is_loopback() {
            " 127.0.0.1은 이 클라이언트 PC 자신입니다. 다른 PC의 호스트에는 호스트의 LAN IP를 입력하세요."
        } else {
            ""
        };
        format!("호스트 연결 실패 ({target}): {error}. {reason}{loopback}")
    })
}

pub fn start_coordinator(config: Config, state: Shared, stop: Arc<AtomicBool>) -> Result<Job, String> {
    if !(1..=MAX_FRAMES).contains(&config.chunk_frames) || !(1..=MAX_WORKERS).contains(&config.max_workers) {
        return Err("구간 크기는 1~100,000프레임, 사용할 PC 수는 1~64대여야 합니다".into());
    }
    if !config.final_video.is_absolute() || !config.final_video.parent().is_some_and(Path::is_dir) {
        return Err("최종 MP4의 저장 위치를 선택하세요".into());
    }
    if config.final_video.extension().and_then(|e| e.to_str()).is_none_or(|e| !e.eq_ignore_ascii_case("mp4")) {
        return Err("최종 파일 이름은 .mp4여야 합니다".into());
    }
    if config.final_video.exists() {
        return Err("최종 MP4가 이미 있습니다. 기존 파일을 보호하려면 새 파일 이름을 선택하세요".into());
    }
    let job = read_job(&config)?;
    if (job.end - job.start).saturating_add(config.chunk_frames - 1) / config.chunk_frames > 100_000 {
        return Err("작업 구간이 너무 많습니다. PC당 작업 구간 크기를 늘리세요".into());
    }
    let listener = TcpListener::bind(&config.address).map_err(|e| format!("서버 열기 {}: {e}", config.address))?;
    listener.set_nonblocking(true).map_err(|e| format!("서버 설정: {e}"))?;
    prepare_output(&job)?;
    {
        let mut s = lock(&state);
        s.chunks = (job.start..job.end)
            .step_by(config.chunk_frames as usize)
            .map(|start| {
                let end = start.saturating_add(config.chunk_frames).min(job.end);
                Chunk {
                    start,
                    end,
                    status: if chunk_complete(&job.output, start, end) { ChunkStatus::Done } else { ChunkStatus::Pending },
                    attempts: 0,
                    worker: String::new(),
                }
            })
            .collect();
        s.total = job.end - job.start;
        s.output = job.output.clone();
        s.messages.clear();
        s.workers = 0;
        s.worker_info.clear();
        s.max_workers = config.max_workers;
        s.dispatch_enabled = false;
        s.assembly_started = false;
        s.final_status = "프레임 렌더링 대기".into();
        s.disabled_workers = config.disabled_workers.clone();
        s.bundle = Arc::new(OnceLock::new());
    }
    log(&state, format!("호스트 시작: {} / {}–{}프레임", config.address, job.start, job.end));
    match listener.local_addr() {
        Ok(address) => {
            if let Err(error) = crate::discovery::start_responder(address, stop.clone()) {
                log(&state, format!("자동 LAN 검색을 사용할 수 없습니다: {error}"));
            }
        }
        Err(error) => log(&state, format!("LAN 검색 주소 확인 실패: {error}")),
    }
    let server_job = job.clone();
    let server_state = state.clone();
    let server_stop = stop.clone();
    let host_config = config.clone();
    thread::spawn(move || {
        while !server_stop.load(Ordering::Relaxed) {
            match listener.accept() {
                Ok((stream, peer)) => {
                    let state = server_state.clone();
                    let job = server_job.clone();
                    let config = host_config.clone();
                    let stop = server_stop.clone();
                    thread::spawn(move || {
                        if let Err(error) = serve_worker(stream, peer, &config, &job, &state, &stop) {
                            log(&state, format!("{peer}: {error}"));
                        }
                    });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => thread::sleep(Duration::from_millis(100)),
                Err(e) => {
                    log(&server_state, format!("연결 수락 오류: {e}"));
                    thread::sleep(Duration::from_secs(1));
                }
            }
        }
        log(&server_state, "서버 종료");
    });
    Ok(job)
}

fn receive(reader: &mut BufReader<TcpStream>) -> Result<Message, String> {
    let mut bytes = Vec::new();
    let n = reader.take((MAX_LINE + 1) as u64).read_until(b'\n', &mut bytes).map_err(|e| e.to_string())?;
    if n == 0 {
        return Err("연결이 닫혔습니다".into());
    }
    if n > MAX_LINE || bytes.last() != Some(&b'\n') {
        return Err("메시지가 너무 길거나 줄 끝이 없습니다".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("잘못된 메시지: {e}"))
}

fn send(stream: &mut TcpStream, message: &Message) -> Result<(), String> {
    let mut bytes = serde_json::to_vec(message).map_err(|e| e.to_string())?;
    bytes.push(b'\n');
    if bytes.len() > MAX_LINE {
        return Err("전송 메시지가 너무 깁니다".into());
    }
    stream.write_all(&bytes).map_err(|e| e.to_string())
}

fn send_path(stream: &mut TcpStream, path: &Path, expected_len: u64) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| format!("전송 파일 열기 {}: {e}", path.display()))?;
    let copied = std::io::copy(&mut file.take(expected_len.saturating_add(1)), stream).map_err(|e| format!("파일 전송: {e}"))?;
    if copied != expected_len {
        return Err(format!("전송 중 파일 크기가 변경됐습니다: {}", path.display()));
    }
    Ok(())
}

fn receive_path(reader: &mut BufReader<TcpStream>, path: &Path, len: u64, sha256: &str) -> Result<(), String> {
    if len > MAX_TRANSFER_FILE || sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("파일 크기 또는 체크섬이 잘못됐습니다".into());
    }
    let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path).map_err(|e| format!("수신 파일 만들기 {}: {e}", path.display()))?;
    let result = (|| -> Result<(), String> {
        let mut remaining = len;
        let mut hash = Sha256::new();
        let mut buf = [0_u8; 128 * 1024];
        while remaining > 0 {
            let limit = usize::try_from(remaining.min(buf.len() as u64)).map_err(|e| e.to_string())?;
            let n = reader.read(&mut buf[..limit]).map_err(|e| format!("파일 수신: {e}"))?;
            if n == 0 {
                return Err("파일 수신 중 연결이 끊겼습니다".into());
            }
            file.write_all(&buf[..n]).map_err(|e| format!("수신 파일 저장: {e}"))?;
            hash.update(&buf[..n]);
            remaining -= n as u64;
        }
        file.sync_all().map_err(|e| format!("수신 파일 확정: {e}"))?;
        if format!("{:x}", hash.finalize()) != sha256.to_ascii_lowercase() {
            return Err("수신 파일 SHA-256이 다릅니다".into());
        }
        Ok(())
    })();
    drop(file);
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

fn send_bundle(stream: &mut TcpStream, bundle: &TransferBundle, job: &Job) -> Result<(), String> {
    send(
        stream,
        &Message::Bundle {
            comp: job.comp.clone(),
            project_len: bundle.project.len() as u64,
            project_sha256: bundle.project_sha256.clone(),
            file_count: bundle.files.len(),
            total: job.total,
            fps_num: job.rate.num,
            fps_den: job.rate.den,
            width: job.width,
            height: job.height,
        },
    )?;
    stream.write_all(&bundle.project).map_err(|e| format!("프로젝트 전송: {e}"))?;
    for file in &bundle.files {
        send(stream, &Message::File { name: file.name.clone(), len: file.len, sha256: file.sha256.clone() })?;
        send_path(stream, &file.source, file.len)?;
    }
    Ok(())
}

struct TempWorkspace {
    root: PathBuf,
    cleaned: bool,
}

impl TempWorkspace {
    fn new() -> Result<Self, String> {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|e| format!("임시 폴더 시간: {e}"))?.as_nanos();
        let root = std::env::temp_dir().join(format!("effectcraft-farm-worker-{}-{now}", std::process::id()));
        std::fs::create_dir(&root).map_err(|e| format!("클라이언트 임시 폴더 만들기: {e}"))?;
        Ok(Self { root, cleaned: false })
    }

    fn cleanup(&mut self) -> Result<(), String> {
        std::fs::remove_dir_all(&self.root).map_err(|e| format!("클라이언트 임시 데이터 삭제 {}: {e}", self.root.display()))?;
        self.cleaned = true;
        Ok(())
    }
}

impl Drop for TempWorkspace {
    fn drop(&mut self) {
        if !self.cleaned {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }
}

fn safe_bundle_name(name: &str) -> bool {
    let path = Path::new(name);
    let mut parts = path.components();
    matches!(parts.next(), Some(std::path::Component::Normal(first)) if first == "media" || first == "fonts")
        && matches!(parts.next(), Some(std::path::Component::Normal(_)))
        && parts.next().is_none()
        && name.len() <= 100
}

fn receive_bundle(reader: &mut BufReader<TcpStream>, config: &mut Config, message: Message) -> Result<(TempWorkspace, Job), String> {
    let Message::Bundle { comp, project_len, project_sha256, file_count, total, fps_num, fps_den, width, height } = message else {
        return Err("전송용 프로젝트 정보가 없습니다".into());
    };
    if project_len > 100_000_000 || file_count > MAX_TRANSFER_FILES || project_sha256.len() != 64 || !project_sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("전송용 프로젝트 크기 또는 정보가 잘못됐습니다".into());
    }
    let workspace = TempWorkspace::new()?;
    let project_path = workspace.root.join("project.ecproj");
    receive_path(reader, &project_path, project_len, &project_sha256)?;
    let mut total_bytes = project_len;
    for _ in 0..file_count {
        let Message::File { name, len, sha256 } = receive(reader)? else {
            return Err("소스 파일 정보가 잘못됐습니다".into());
        };
        if !safe_bundle_name(&name) {
            return Err("허용되지 않는 소스 파일 이름입니다".into());
        }
        total_bytes = total_bytes.checked_add(len).ok_or("전송 크기가 너무 큽니다")?;
        if total_bytes > MAX_TRANSFER_TOTAL {
            return Err("전송 합계가 크기 제한을 넘습니다".into());
        }
        let target = workspace.root.join(&name);
        let parent = target.parent().ok_or("소스 폴더가 없습니다")?;
        std::fs::create_dir_all(parent).map_err(|e| format!("소스 폴더 만들기: {e}"))?;
        receive_path(reader, &target, len, &sha256)?;
    }
    let output = workspace.root.join("frames");
    std::fs::create_dir(&output).map_err(|e| format!("임시 프레임 폴더 만들기: {e}"))?;
    config.project = project_path;
    config.output = output;
    config.comp = comp;
    config.range_start = 0;
    config.range_end = total;
    let job = read_job_without_preflight(config)?;
    if job.total != total || job.rate.num != fps_num || job.rate.den != fps_den || job.width != width || job.height != height {
        return Err("호스트 작업 설정이 전송된 프로젝트와 다릅니다".into());
    }
    Ok((workspace, job))
}

fn serve_worker(mut stream: TcpStream, peer: SocketAddr, config: &Config, job: &Job, state: &Shared, stop: &AtomicBool) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(3600))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let (name, given_token, protocol) = match receive(&mut reader)? {
        Message::Hello { token, name, protocol, .. } => (name, token, protocol),
        _ => return Err("인증 메시지가 필요합니다".into()),
    };
    if protocol != PROTOCOL_VERSION {
        send(
            &mut stream,
            &Message::Error {
                detail: "호스트와 클라이언트 앱 버전이 다릅니다. 모든 PC를 최신 버전으로 업데이트하세요".into()
            },
        )?;
        return Err("호환되지 않는 클라이언트 버전".into());
    }
    if given_token != config.token || name.len() > 80 {
        send(&mut stream, &Message::Error { detail: "접속 암호가 올바르지 않습니다".into() })?;
        return Err("접속 정보 검증 실패".into());
    }
    let ip = peer.ip().to_string();
    let label = format!("{name} @ {ip}");
    let worker_id: Result<u64, &'static str> = {
        let mut s = lock(state);
        if s.workers >= MAX_WORKERS || s.next_worker_id == u64::MAX {
            Err("호스트 연결 한도를 넘었습니다")
        } else if s.worker_info.values().any(|worker| worker.connected && worker.ip == ip) {
            Err("이 클라이언트는 이미 연결되어 있습니다")
        } else {
            if s.worker_info.len() >= 128 {
                s.worker_info.retain(|_, info| info.connected);
            }
            s.next_worker_id += 1;
            let id = s.next_worker_id;
            s.workers += 1;
            let enabled = !s.disabled_workers.contains(&ip) && !s.disabled_workers.contains(&label);
            s.worker_info.insert(
                id,
                WorkerInfo { id, ip: ip.clone(), label: label.clone(), enabled, connected: true, assigned: None, current_done: 0, completed_frames: 0 },
            );
            Ok(id)
        }
    };
    let worker_id = match worker_id {
        Ok(id) => id,
        Err(detail) => {
            send(&mut stream, &Message::Error { detail: detail.into() })?;
            return Err(detail.into());
        }
    };
    if let Err(error) = send(&mut stream, &Message::Ready { comp: job.comp.clone() }) {
        let mut s = lock(state);
        s.workers = s.workers.saturating_sub(1);
        s.worker_info.remove(&worker_id);
        return Err(error);
    }
    log(state, format!("{label} 연결"));
    let mut assigned: Option<(i64, i64)> = None;
    let mut prepared = false;
    let mut received_frames = 0_i64;
    let result = (|| -> Result<(), String> {
        loop {
            match receive(&mut reader) {
                Ok(Message::Request) if assigned.is_none() => {
                    if stop.load(Ordering::Relaxed) {
                        send(&mut stream, &Message::Finished)?;
                        break Ok(());
                    }
                    let may_prepare = {
                        let s = lock(state);
                        s.dispatch_enabled
                            && s.worker_info.get(&worker_id).is_some_and(|worker| worker.enabled)
                            && s.chunks.iter().any(|chunk| chunk.status == ChunkStatus::Pending)
                            && s.chunks.iter().filter(|chunk| chunk.status == ChunkStatus::Running).count() < s.max_workers
                    };
                    if may_prepare && !prepared {
                        log(state, format!("{label}: 프로젝트와 소스 전송 중"));
                        let cache = { lock(state).bundle.clone() };
                        let bundle = cache.get_or_init(|| build_bundle(config, job).map(Arc::new)).clone();
                        let transfer = bundle.and_then(|bundle| send_bundle(&mut stream, &bundle, job));
                        if let Err(error) = transfer {
                            let _ = send(&mut stream, &Message::Error { detail: error.clone() });
                            return Err(error);
                        }
                        prepared = true;
                        log(state, format!("{label}: 소스 전송 완료"));
                        continue;
                    }
                    let next = {
                        let mut s = lock(state);
                        let finished = s.chunks.iter().all(|c| matches!(c.status, ChunkStatus::Done | ChunkStatus::Failed));
                        let enabled = prepared && s.dispatch_enabled && s.worker_info.get(&worker_id).is_some_and(|w| w.enabled);
                        let available = s.chunks.iter().filter(|c| c.status == ChunkStatus::Running).count() < s.max_workers;
                        if finished {
                            None
                        } else if enabled
                            && available
                            && let Some(chunk) = s.chunks.iter_mut().find(|c| c.status == ChunkStatus::Pending)
                        {
                            chunk.status = ChunkStatus::Running;
                            chunk.worker = label.clone();
                            let range = (chunk.start, chunk.end);
                            if let Some(info) = s.worker_info.get_mut(&worker_id) {
                                info.assigned = Some(range);
                                info.current_done = 0;
                            }
                            Some(range)
                        } else {
                            None
                        }
                    };
                    match next {
                        Some((start, end)) => {
                            assigned = next;
                            received_frames = 0;
                            send(&mut stream, &Message::Assign { start, end, fps_num: job.rate.num, fps_den: job.rate.den, comp: job.comp.clone() })?;
                        }
                        None => {
                            let s = lock(state);
                            let finished = s.dispatch_enabled && s.chunks.iter().all(|c| matches!(c.status, ChunkStatus::Done | ChunkStatus::Failed));
                            drop(s);
                            send(&mut stream, if finished { &Message::Finished } else { &Message::Wait })?;
                            if finished {
                                break Ok(());
                            }
                        }
                    }
                }
                Ok(Message::Progress { start, end, done }) if assigned == Some((start, end)) => {
                    if done < 0 || done > end - start {
                        break Err("클라이언트 진행률이 작업 범위를 벗어났습니다".into());
                    }
                    let mut s = lock(state);
                    if let Some(info) = s.worker_info.get_mut(&worker_id) {
                        if done < info.current_done {
                            break Err("클라이언트 진행률이 역행했습니다".into());
                        }
                        info.current_done = done;
                    }
                }
                Ok(Message::Frame { frame, len, sha256 }) if assigned.is_some() => {
                    let (start, end) = assigned.ok_or("작업 구간이 없습니다")?;
                    if frame != start.saturating_add(received_frames) || frame >= end || len > 512 * 1024 * 1024 {
                        break Err("프레임 번호 또는 크기가 작업 지시와 다릅니다".into());
                    }
                    let target = frame_path(&job.output, frame);
                    let partial = job.output.join(format!("frame_{frame:05}.worker_{worker_id}.partial"));
                    receive_path(&mut reader, &partial, len, &sha256)?;
                    if !valid_png(&partial) {
                        let _ = std::fs::remove_file(&partial);
                        break Err(format!("수신 프레임 {frame}이 손상됐습니다"));
                    }
                    if target.exists() {
                        std::fs::remove_file(&target).map_err(|e| format!("이전 프레임 교체: {e}"))?;
                    }
                    std::fs::rename(&partial, &target).map_err(|e| format!("프레임 확정: {e}"))?;
                    received_frames += 1;
                    let mut s = lock(state);
                    if let Some(info) = s.worker_info.get_mut(&worker_id) {
                        info.current_done = info.current_done.max(received_frames);
                    }
                }
                Ok(Message::Complete { start, end, ok, detail }) if assigned == Some((start, end)) => {
                    let verified = ok && received_frames == end - start && chunk_complete(&job.output, start, end);
                    {
                        let mut s = lock(state);
                        if let Some(chunk) = s.chunks.iter_mut().find(|c| c.start == start && c.end == end) {
                            chunk.attempts = chunk.attempts.saturating_add(1);
                            chunk.status = if verified {
                                ChunkStatus::Done
                            } else if chunk.attempts >= 3 {
                                ChunkStatus::Failed
                            } else {
                                ChunkStatus::Pending
                            };
                            if chunk.status == ChunkStatus::Failed {
                                s.final_status = format!("{start}–{end} 구간이 세 번 실패했습니다");
                            }
                            s.note(format!("{label}: {start}–{end} {} {detail}", if verified { "완료" } else { "실패" }));
                        }
                        if let Some(info) = s.worker_info.get_mut(&worker_id) {
                            if verified {
                                info.completed_frames = info.completed_frames.saturating_add(end - start);
                            }
                            info.assigned = None;
                            info.current_done = 0;
                        }
                    }
                    assigned = None;
                    if verified {
                        maybe_start_assembly(config, job, state);
                    }
                }
                Ok(Message::Disconnect) => break Ok(()),
                Ok(Message::Error { detail }) => break Err(detail),
                Ok(_) => break Err("예상하지 않은 작업 메시지입니다".into()),
                Err(e) => break Err(e),
            }
        }
    })();
    if let Some((start, end)) = assigned {
        let mut s = lock(state);
        if let Some(chunk) = s.chunks.iter_mut().find(|c| c.start == start && c.end == end && c.status == ChunkStatus::Running) {
            chunk.status = ChunkStatus::Pending;
        }
    }
    let mut s = lock(state);
    s.workers = s.workers.saturating_sub(1);
    if let Some(info) = s.worker_info.get_mut(&worker_id) {
        info.connected = false;
        info.assigned = None;
        info.current_done = 0;
    }
    result
}

pub fn set_worker_enabled(state: &Shared, id: u64, enabled: bool) -> Option<String> {
    let mut s = lock(state);
    let worker = s.worker_info.get_mut(&id)?;
    worker.enabled = enabled;
    let ip = worker.ip.clone();
    let label = worker.label.clone();
    s.disabled_workers.retain(|saved| saved != &ip && saved != &label);
    if !enabled {
        s.disabled_workers.push(ip.clone());
    }
    Some(ip)
}

pub fn begin_render(config: &Config, job: &Job, state: &Shared) {
    {
        let mut s = lock(state);
        s.dispatch_enabled = true;
        s.final_status = "프레임 렌더링 중".into();
        s.note("호스트가 렌더링을 시작했습니다");
    }
    maybe_start_assembly(config, job, state);
}

pub fn run_worker(config: Config, state: Shared, stop: Arc<AtomicBool>) -> Result<(), String> {
    if !config.cli.is_file() {
        return Err(format!("렌더 CLI 파일이 없습니다: {}", config.cli.display()));
    }
    let mut stream = connect_to_host(&config.address)?;
    stream.set_read_timeout(Some(Duration::from_secs(3600))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "worker".into());
    send(&mut stream, &Message::Hello { token: config.token.clone(), name, protocol: PROTOCOL_VERSION })?;
    let comp = match receive(&mut reader)? {
        Message::Ready { comp } => comp,
        Message::Error { detail } => return Err(detail),
        _ => return Err("서버 인증 응답이 잘못됐습니다".into()),
    };
    let mut config = config;
    let mut workspace: Option<TempWorkspace> = None;
    let mut job: Option<Job> = None;
    log(&state, format!("호스트 연결 완료 · {comp} · 렌더 지시 대기"));
    while !stop.load(Ordering::Relaxed) {
        send(&mut stream, &Message::Request)?;
        match receive(&mut reader)? {
            message @ Message::Bundle { .. } if workspace.is_none() => {
                log(&state, "호스트에서 프로젝트·소스 수신 중");
                let (received_workspace, received_job) = receive_bundle(&mut reader, &mut config, message)?;
                if received_job.comp != comp {
                    return Err("호스트 컴포지션 지시가 변경됐습니다".into());
                }
                log(&state, format!("렌더 준비 완료: {}", received_job.comp));
                job = Some(received_job);
                workspace = Some(received_workspace);
            }
            Message::Assign { start, end, fps_num, fps_den, comp } => {
                let job = job.as_ref().ok_or("호스트 프로젝트를 아직 받지 못했습니다")?;
                if start < 0 || end <= start || end > job.total || fps_num != job.rate.num || fps_den != job.rate.den || comp != job.comp {
                    return Err("서버 작업 범위 또는 설정이 프로젝트와 다릅니다".into());
                }
                log(&state, format!("렌더링: {start}–{end}"));
                let result = render_chunk(&config, job, start, end, |done| {
                    lock(&state).local_progress = Some((start, end, done));
                    send(&mut stream, &Message::Progress { start, end, done })
                });
                let (ok, detail) = match result {
                    Ok(()) => (true, "".into()),
                    Err(e) => (false, e.chars().take(1800).collect::<String>()),
                };
                if ok {
                    for frame in start..end {
                        let path = frame_path(&job.output, frame);
                        let len = std::fs::metadata(&path).map_err(|e| format!("전송 프레임 정보: {e}"))?.len();
                        if len > 512 * 1024 * 1024 {
                            return Err(format!("프레임 {frame}이 512MB를 넘습니다"));
                        }
                        let sha256 = file_sha256(&path)?;
                        send(&mut stream, &Message::Frame { frame, len, sha256 })?;
                        send_path(&mut stream, &path, len)?;
                        std::fs::remove_file(&path).map_err(|e| format!("전송한 임시 프레임 삭제: {e}"))?;
                    }
                }
                send(&mut stream, &Message::Complete { start, end, ok, detail: detail.clone() })?;
                if ok {
                    log(&state, format!("구간 {start}–{end} 완료 · 다음 작업 대기"));
                } else {
                    log(&state, format!("구간 {start}–{end} 실패: {detail}"));
                }
            }
            Message::Wait => thread::sleep(Duration::from_secs(1)),
            Message::Finished => {
                log(&state, "모든 작업 완료");
                break;
            }
            Message::Error { detail } => return Err(detail),
            _ => return Err("서버 응답이 잘못됐습니다".into()),
        }
    }
    if stop.load(Ordering::Relaxed) {
        let _ = send(&mut stream, &Message::Disconnect);
    }
    if let Some(mut workspace) = workspace {
        workspace.cleanup()?;
    }
    log(&state, "클라이언트 임시 프로젝트·소스·프레임 삭제 완료");
    Ok(())
}

fn render_chunk(config: &Config, job: &Job, start: i64, end: i64, mut progress: impl FnMut(i64) -> Result<(), String>) -> Result<(), String> {
    // A tiny bias below each boundary ensures the CLI's f64-to-Tick conversion
    // cannot advance a boundary by one tick and skip its frame.
    let start_s = format!("{:.9}", (job.rate.tick_of(start).seconds() - 0.000_001).max(0.0));
    let end_s = format!("{:.9}", (job.rate.tick_of(end).seconds() - 0.000_001).max(0.0));
    let mut command = Command::new(&config.cli);
    command
        .arg("render")
        .arg(&config.project)
        .arg("--comp")
        .arg(&job.comp)
        .arg("--out")
        .arg(config.output.join("frame_[#####].png"))
        .arg("--format")
        .arg("png")
        .arg("--start")
        .arg(start_s)
        .arg("--end")
        .arg(end_s);
    if config.gpu {
        command.arg("--gpu");
    }
    if let Some(root) = config.project.parent() {
        let fonts = root.join("fonts");
        if fonts.is_dir() {
            command.env("EFFECTCRAFT_FONT_DIRS", fonts);
        }
    }
    let stderr_path = config.output.join(format!("cli_{start}_{end}.stderr"));
    let stderr_file = std::fs::File::create(&stderr_path).map_err(|e| format!("CLI 오류 기록 파일 만들기: {e}"))?;
    command.stdout(Stdio::null()).stderr(Stdio::from(stderr_file));
    let mut child = command.spawn().map_err(|e| format!("렌더 CLI 실행: {e}"))?;
    let mut next = start;
    let mut last_progress = -1_i64;
    let status = loop {
        let current = match child.try_wait() {
            Ok(status) => status,
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("렌더 CLI 상태 확인: {error}"));
            }
        };
        while next < end && valid_png(&frame_path(&config.output, next)) {
            next += 1;
        }
        let done = next - start;
        if done != last_progress {
            if let Err(error) = progress(done) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            last_progress = done;
        }
        if let Some(status) = current {
            break status;
        }
        thread::sleep(Duration::from_millis(400));
    };
    if !status.success() {
        let mut file = std::fs::File::open(&stderr_path).map_err(|e| format!("CLI 오류 기록 읽기: {e}"))?;
        let len = file.metadata().map_err(|e| format!("CLI 오류 기록 정보: {e}"))?.len();
        file.seek(SeekFrom::Start(len.saturating_sub(4000))).map_err(|e| format!("CLI 오류 기록 위치: {e}"))?;
        let mut bytes = Vec::new();
        file.take(4000).read_to_end(&mut bytes).map_err(|e| format!("CLI 오류 기록 읽기: {e}"))?;
        let tail = String::from_utf8_lossy(&bytes);
        let _ = std::fs::remove_file(&stderr_path);
        return Err(format!("CLI 종료 {status}: {tail}"));
    }
    let _ = std::fs::remove_file(&stderr_path);
    if !chunk_complete(&config.output, start, end) {
        return Err("CLI는 종료됐지만 일부 PNG 프레임이 없거나 손상됐습니다".into());
    }
    Ok(())
}

fn maybe_start_assembly(config: &Config, job: &Job, state: &Shared) {
    {
        let mut s = lock(state);
        if s.assembly_started || s.chunks.is_empty() || !s.chunks.iter().all(|c| c.status == ChunkStatus::Done) {
            return;
        }
        s.assembly_started = true;
        s.final_status = "호스트에서 최종 MP4 합치는 중".into();
    }
    let config = config.clone();
    let job = job.clone();
    let state = state.clone();
    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assemble_final(&config, &job)))
            .unwrap_or_else(|_| Err("최종 영상 합치기 중 예상하지 못한 오류가 발생했습니다".into()));
        let mut s = lock(&state);
        match result {
            Ok(()) => {
                s.final_status = format!("최종 영상 완료: {}", config.final_video.display());
                let message = s.final_status.clone();
                s.note(message);
            }
            Err(e) => {
                s.final_status = format!("최종 영상 실패: {e}");
                let message = s.final_status.clone();
                s.note(message);
            }
        }
    });
}

fn assembly_project(config: &Config, job: &Job) -> Result<Project, String> {
    if std::fs::metadata(&config.project).map_err(|e| format!("원본 프로젝트 정보: {e}"))?.len() > 100_000_000 {
        return Err("원본 프로젝트가 100MB 제한을 넘습니다".into());
    }
    let bytes = std::fs::read(&config.project).map_err(|e| format!("원본 프로젝트 다시 읽기: {e}"))?;
    if project_hash(&bytes) != job.hash {
        return Err("렌더링 중 원본 프로젝트 파일이 변경됐습니다".into());
    }
    let text = std::str::from_utf8(&bytes).map_err(|e| format!("프로젝트 문자 인코딩: {e}"))?;
    let mut project = Project::from_json(text).map_err(|e| format!("프로젝트 열기: {e}"))?;
    let matches: Vec<_> =
        project.comps().filter(|(id, _)| project.item(**id).is_some_and(|item| item.name == job.comp)).map(|(id, comp)| (*id, comp.clone())).collect();
    let [(comp_id, comp)] = matches.as_slice() else {
        return Err("원본 컴포지션을 찾을 수 없습니다".into());
    };
    if comp.frame_rate != job.rate || comp.width != job.width || comp.height != job.height || job.end > job.total {
        return Err("프로젝트의 영상 설정이 렌더 작업과 다릅니다".into());
    }
    if !chunk_complete(&job.output, job.start, job.end) {
        return Err("최종 합치기 전 프레임 파일 검사에 실패했습니다".into());
    }
    let files: Vec<String> = (job.start..job.end).map(|frame| frame_path(&job.output, frame).to_string_lossy().into_owned()).collect();
    let first = files.first().ok_or("렌더링한 프레임이 없습니다")?.clone();
    let mut footage = Footage {
        path: first,
        kind: FootageKind::Sequence,
        width: job.width,
        height: job.height,
        pixel_aspect: comp.pixel_aspect,
        frame_rate: job.rate,
        has_video: true,
        has_audio: false,
        sequence: files,
        ..Footage::default()
    };
    footage.sync_sequence_duration();
    let item = project.add_item("Farm Render Frames", Default::default(), None, ItemKind::Footage(footage.clone()));
    let mut layer = effectcraft_project::build::layer(
        &mut project,
        comp,
        "Farm Render Frames",
        LayerSource::Footage { item },
        (job.width, job.height),
        Some(footage.duration),
    );
    layer.start_time = job.rate.tick_of(job.start);
    layer.in_point = layer.start_time;
    layer.out_point = job.rate.tick_of(job.end);
    layer.switches.audio = false;
    let target = project.comp_mut(*comp_id).ok_or("원본 컴포지션이 사라졌습니다")?;
    for existing in &mut target.layers {
        existing.switches.video = false;
    }
    target.layers.insert(0, layer);
    Ok(project)
}

fn assemble_final(config: &Config, job: &Job) -> Result<(), String> {
    if config.final_video.exists() {
        return Err(format!("최종 파일이 이미 있습니다. 덮어쓰지 않았습니다: {}", config.final_video.display()));
    }
    let project = assembly_project(config, job)?;
    let scratch = std::env::temp_dir().join(format!(
        "effectcraft-farm-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos(),
    ));
    std::fs::create_dir(&scratch).map_err(|e| format!("임시 프로젝트 폴더 만들기: {e}"))?;
    let scratch_project = scratch.join("assembly.ecproj");
    let json = project.to_file_json().map_err(|e| format!("임시 프로젝트 생성: {e}"))?;
    std::fs::write(&scratch_project, json).map_err(|e| format!("임시 프로젝트 저장: {e}"))?;
    let parent = config.final_video.parent().ok_or("최종 영상 폴더가 없습니다")?;
    let name = config.final_video.file_stem().and_then(|n| n.to_str()).ok_or("최종 파일 이름을 읽을 수 없습니다")?;
    let partial = parent.join(format!("{name}.farm-partial-{}-{}.mp4", std::process::id(), job.start));
    let start_s = format!("{:.9}", (job.rate.tick_of(job.start).seconds() - 0.000_001).max(0.0));
    let end_s = format!("{:.9}", (job.rate.tick_of(job.end).seconds() - 0.000_001).max(0.0));
    let mut command = Command::new(&config.cli);
    command
        .arg("render")
        .arg(&scratch_project)
        .arg("--comp")
        .arg(&job.comp)
        .arg("--out")
        .arg(&partial)
        .arg("--format")
        .arg("h264")
        .arg("--start")
        .arg(start_s)
        .arg("--end")
        .arg(end_s)
        .arg("--audio")
        .arg("auto");
    if config.gpu {
        command.arg("--gpu");
    }
    let output = command.output().map_err(|e| format!("최종 렌더 CLI 실행: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(800).collect::<String>().chars().rev().collect();
        return Err(format!("최종 영상 출력 실패 ({}): {tail}; 임시 프로젝트: {}", output.status, scratch_project.display()));
    }
    if std::fs::metadata(&partial).map_err(|e| format!("최종 임시 파일 검사: {e}"))?.len() < 128 {
        return Err(format!("최종 임시 MP4가 비어 있습니다: {}", partial.display()));
    }
    std::fs::rename(&partial, &config.final_video).map_err(|e| format!("최종 파일 이름 확정: {e}; 임시 파일: {}", partial.display()))?;
    let _ = std::fs::remove_file(&scratch_project);
    let _ = std::fs::remove_dir(&scratch);
    Ok(())
}

pub fn worker_snapshot(state: &Shared) -> (Vec<WorkerInfo>, String) {
    let s = lock(state);
    (s.worker_info.values().cloned().collect(), s.final_status.clone())
}

pub fn snapshot(state: &Shared) -> (i64, i64, usize, usize, Vec<String>, PathBuf) {
    let s = lock(state);
    let mut done = 0_i64;
    let mut failed = 0_usize;
    let running = s.worker_info.values().filter_map(|worker| worker.assigned.map(|range| (range, worker.current_done))).collect::<BTreeMap<_, _>>();
    for chunk in &s.chunks {
        match chunk.status {
            ChunkStatus::Done => done = done.saturating_add(chunk.end - chunk.start),
            ChunkStatus::Running => {
                if let Some(progress) = running.get(&(chunk.start, chunk.end)) {
                    done = done.saturating_add((*progress).clamp(0, chunk.end - chunk.start));
                }
            }
            ChunkStatus::Failed => failed += 1,
            ChunkStatus::Pending => {}
        }
    }
    (done.min(s.total), s.total, s.workers, failed, s.messages.clone(), s.output.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_color::Label;
    use effectcraft_project::{Comp, ItemKind};
    use effectcraft_time::Tick;

    fn receive_test_bundle(reader: &mut BufReader<TcpStream>) {
        let Message::Bundle { project_len, file_count, .. } = receive(reader).unwrap() else { panic!("bundle expected") };
        let mut project = vec![0_u8; project_len as usize];
        reader.read_exact(&mut project).unwrap();
        assert!(Project::from_json(std::str::from_utf8(&project).unwrap()).is_ok());
        for _ in 0..file_count {
            let Message::File { len, .. } = receive(reader).unwrap() else { panic!("file expected") };
            std::io::copy(&mut reader.take(len), &mut std::io::sink()).unwrap();
        }
    }

    #[test]
    fn connection_errors_explain_invalid_address_and_closed_local_port() {
        let invalid = connect_to_host("not an address").err().unwrap();
        assert!(invalid.contains("LAN IP"));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let closed = connect_to_host(&address.to_string()).err().unwrap();
        assert!(closed.contains("호스트 열기"));
        assert!(closed.contains("127.0.0.1"));
    }

    fn send_test_frames(stream: &mut TcpStream, start: i64, end: i64) {
        for frame in start..end {
            let mut bytes = PNG_HEADER.to_vec();
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(b"IEND");
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            send(stream, &Message::Frame { frame, len: bytes.len() as u64, sha256: project_hash(&bytes) }).unwrap();
            stream.write_all(&bytes).unwrap();
        }
    }

    #[test]
    fn partial_final_frame_counts_without_requiring_aligned_duration() {
        let rate = FrameRate::new(30_000, 1001);
        let boundary = rate.tick_of(70);
        assert_eq!(comp_frame_count(rate, boundary).unwrap(), 70);
        assert_eq!(comp_frame_count(rate, Tick(boundary.0 + 1)).unwrap(), 71);
        assert!(comp_frame_count(rate, Tick::ZERO).is_err());
    }

    #[test]
    fn missing_media_and_sequence_frame_are_reported() {
        let project = std::env::temp_dir().join("farm-preflight-example.ecproj");
        let mut footage = Footage { path: "missing-video.mov".into(), ..Footage::default() };
        footage.sequence.push("missing-frame.png".into());
        let mut missing = BTreeSet::new();
        check_footage(&footage, &project, &mut missing);
        assert_eq!(missing.len(), 2);
        assert!(missing.iter().any(|entry| entry.contains("missing-frame.png")));
    }

    #[test]
    fn unavailable_font_is_reported_before_render() {
        let mut missing = BTreeSet::new();
        check_font("EffectCraft Definitely Missing Test Font 8765", "Regular", &mut missing);
        assert!(missing.iter().any(|entry| entry.contains("EffectCraft Definitely Missing Test Font 8765")));
    }

    #[test]
    fn chunk_partition_has_no_gaps() {
        let chunk_frames = 300_i64;
        let ranges: Vec<_> = (0_i64..15_800).step_by(chunk_frames as usize).map(|a| (a, (a + chunk_frames).min(15_800))).collect();
        assert_eq!(ranges.first(), Some(&(0, 300)));
        assert_eq!(ranges.last(), Some(&(15_600, 15_800)));
        assert!(ranges.windows(2).all(|w| w[0].1 == w[1].0));
    }

    #[test]
    fn invalid_png_is_not_complete() {
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-test-{}", std::process::id()));
        assert!(!chunk_complete(&dir, 0, 1));
    }

    #[test]
    fn errors_are_saved_to_a_diagnostic_log() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-log-{}-{suffix}", std::process::id()));
        let path = dir.join("render-farm.log");
        let mut state = State::with_log_path(Some(path.clone()));
        state.note("CLI 종료 실패: sample error");
        assert!(std::fs::read_to_string(&path).unwrap().contains("CLI 종료 실패: sample error"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn client_workspace_deletes_received_files_and_rejects_traversal_names() {
        assert!(safe_bundle_name("media/asset_000001.png"));
        assert!(!safe_bundle_name("media/../secret.txt"));
        assert!(!safe_bundle_name("C:/secret.txt"));
        let mut workspace = TempWorkspace::new().unwrap();
        let path = workspace.root.join("project.ecproj");
        std::fs::write(&path, b"temporary").unwrap();
        workspace.cleanup().unwrap();
        assert!(!path.exists());
    }

    #[test]
    fn chunk_time_bias_keeps_exact_frames() {
        let rate = FrameRate::FPS_60;
        for frame in [0, 300, 15_600, 15_800] {
            let seconds = (rate.tick_of(frame).seconds() - 0.000_001).max(0.0);
            let tick = effectcraft_time::Tick::from_seconds_f64(seconds);
            let first = rate.frame_at(tick);
            let ceil = if rate.tick_of(first) < tick { first + 1 } else { first };
            assert_eq!(ceil, frame);
        }
    }

    #[test]
    fn work_area_uses_ceil_frame_boundaries_and_invalid_area_falls_back() {
        let rate = FrameRate::FPS_60;
        let duration = rate.tick_of(1000);
        assert_eq!(work_frame_range(rate, duration, (rate.tick_of(100), rate.tick_of(900)), 1000), (100, 900));
        assert_eq!(work_frame_range(rate, duration, (rate.tick_of(100) + Tick::from_seconds_f64(0.001), rate.tick_of(900)), 1000), (101, 900));
        assert_eq!(work_frame_range(rate, duration, (rate.tick_of(900), rate.tick_of(100)), 1000), (0, 1000));
    }

    #[test]
    fn project_validation_and_manifest_reject_mixed_jobs() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let project_path = dir.join("test.ecproj");
        let mut project = Project::default();
        let mut comp = Comp::new(64, 64, FrameRate::FPS_60, Tick::from_seconds_f64(2.0));
        comp.work_area = (FrameRate::FPS_60.tick_of(30), FrameRate::FPS_60.tick_of(90));
        project.add_item("Test", Label::None, None, ItemKind::Comp(Arc::new(comp)));
        std::fs::write(&project_path, project.to_json()).unwrap();
        let catalog = list_compositions(&project_path).unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].name, "Test");
        assert_eq!(catalog[0].frames, 120);
        assert_eq!((catalog[0].work_start, catalog[0].work_end), (30, 90));
        let config = Config {
            project: project_path,
            cli: std::env::current_exe().unwrap(),
            output: dir.clone(),
            comp: "Test".into(),
            address: "127.0.0.1:0".into(),
            token: "test-pass-123".into(),
            gpu: false,
            range_start: 30,
            range_end: 90,
            chunk_frames: 30,
            max_workers: 2,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&config).unwrap();
        assert_eq!((job.total, job.width, job.height), (120, 64, 64));
        assert_eq!((job.start, job.end), (30, 90));
        prepare_output(&job).unwrap();
        prepare_output(&job).unwrap();
        let mut other = job.clone();
        other.comp = "Other".into();
        assert!(prepare_output(&other).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tcp_worker_receives_and_completes_range() {
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-tcp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut project = Project::default();
        project.add_item("Test", Label::None, None, ItemKind::Comp(Arc::new(Comp::new(2, 2, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(2)))));
        let project_path = dir.join("test.ecproj");
        std::fs::write(&project_path, project.to_json()).unwrap();
        let config = Config {
            project: project_path,
            cli: std::env::current_exe().unwrap(),
            output: dir.clone(),
            comp: "Test".into(),
            address: "127.0.0.1:0".into(),
            token: "password".into(),
            gpu: false,
            range_start: 0,
            range_end: 2,
            chunk_frames: 2,
            max_workers: 1,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&config).unwrap();
        let state = Arc::new(Mutex::new(State {
            chunks: vec![Chunk { start: 0, end: 2, status: ChunkStatus::Pending, attempts: 0, worker: String::new() }],
            total: 2,
            max_workers: 1,
            assembly_started: true,
            output: dir.clone(),
            ..State::default()
        }));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_state = state.clone();
        let server_job = job.clone();
        let server_config = config.clone();
        let server = thread::spawn(move || {
            let (stream, peer) = listener.accept().unwrap();
            serve_worker(stream, peer, &server_config, &server_job, &server_state, &AtomicBool::new(false))
        });
        let mut client = TcpStream::connect(address).unwrap();
        let mut reader = BufReader::new(client.try_clone().unwrap());
        send(&mut client, &Message::Hello { token: "password".into(), name: "test-worker".into(), protocol: PROTOCOL_VERSION }).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Ready { comp }) if comp == "Test"));
        send(&mut client, &Message::Request).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Wait)));
        lock(&state).dispatch_enabled = true;
        let worker_id = worker_snapshot(&state).0.first().unwrap().id;
        assert_eq!(set_worker_enabled(&state, worker_id, false).as_deref(), Some("127.0.0.1"));
        assert_eq!(lock(&state).disabled_workers, vec!["127.0.0.1"]);
        send(&mut client, &Message::Request).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Wait)));
        assert_eq!(set_worker_enabled(&state, worker_id, true).as_deref(), Some("127.0.0.1"));
        assert!(lock(&state).disabled_workers.is_empty());
        send(&mut client, &Message::Request).unwrap();
        receive_test_bundle(&mut reader);
        send(&mut client, &Message::Request).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Assign { start: 0, end: 2, .. })));
        send(&mut client, &Message::Progress { start: 0, end: 2, done: 1 }).unwrap();
        for _ in 0..100 {
            if snapshot(&state).0 == 1 {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(snapshot(&state).0, 1);
        assert_eq!(worker_snapshot(&state).0.first().unwrap().current_done, 1);
        send_test_frames(&mut client, 0, 2);
        send(&mut client, &Message::Complete { start: 0, end: 2, ok: true, detail: String::new() }).unwrap();
        send(&mut client, &Message::Request).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Finished)));
        assert!(server.join().unwrap().is_ok());
        assert_eq!(snapshot(&state).0, 2);
        assert_eq!(worker_snapshot(&state).0.first().unwrap().completed_frames, 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn client_uses_host_selected_composition() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-host-comp-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let project_path = dir.join("project.ecproj");
        let mut project = Project::default();
        let comp = Comp::new(64, 64, FrameRate::FPS_60, Tick::from_seconds_f64(1.0));
        let comp_id = project.add_item("Host Choice", Label::None, None, ItemKind::Comp(Arc::new(comp.clone())));
        let media_path = dir.join("source.png");
        std::fs::write(&media_path, b"source fixture").unwrap();
        let footage = Footage { path: media_path.to_string_lossy().into_owned(), kind: FootageKind::Still, ..Footage::default() };
        let footage_id = project.add_item("Source", Label::None, None, ItemKind::Footage(footage));
        let layer = effectcraft_project::build::layer(&mut project, &comp, "Source", LayerSource::Footage { item: footage_id }, (64, 64), Some(comp.duration));
        project.comp_mut(comp_id).unwrap().layers.push(layer);
        std::fs::write(&project_path, project.to_json()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = Config {
            project: project_path.clone(),
            cli: project_path,
            output: dir.clone(),
            comp: "Host Choice".into(),
            address: address.to_string(),
            token: "test-password".into(),
            gpu: false,
            range_start: 0,
            range_end: 0,
            chunk_frames: 60,
            max_workers: 1,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&config).unwrap();
        let bundle = build_bundle(&config, &job).unwrap();
        assert_eq!(bundle.files.len(), 1);
        let transferred = Project::from_json(std::str::from_utf8(&bundle.project).unwrap()).unwrap();
        let rewritten = transferred.item(footage_id).unwrap();
        assert!(matches!(&rewritten.kind, ItemKind::Footage(f) if f.path.starts_with("media/asset_")));
        let state = Arc::new(Mutex::new(State {
            chunks: vec![Chunk { start: 0, end: 60, status: ChunkStatus::Pending, attempts: 0, worker: String::new() }],
            dispatch_enabled: true,
            max_workers: 1,
            output: dir.clone(),
            ..State::default()
        }));
        let stop = Arc::new(AtomicBool::new(false));
        let server_state = state.clone();
        let server_stop = stop.clone();
        let host_config = config.clone();
        let server = thread::spawn(move || {
            let (stream, peer) = listener.accept().unwrap();
            serve_worker(stream, peer, &host_config, &job, &server_state, &server_stop)
        });
        config.comp = "stale client selection".into();
        let client_state = state.clone();
        let client_stop = stop.clone();
        let client = thread::spawn(move || run_worker(config, client_state, client_stop));
        for _ in 0..100 {
            if lock(&state).messages.iter().any(|message| message == "렌더 준비 완료: Host Choice") {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(lock(&state).messages.iter().any(|message| message == "렌더 준비 완료: Host Choice"));
        stop.store(true, Ordering::Relaxed);
        assert!(client.join().unwrap().is_ok());
        assert!(server.join().unwrap().is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn real_cli_renders_from_host_transfer_without_client_project_or_shared_folder() {
        let Some(cli) = std::env::var_os("EFFECTCRAFT_FARM_TEST_CLI").map(PathBuf::from) else { return };
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-transfer-render-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let source = dir.join("source.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([60, 110, 170, 255])).save(&source).unwrap();
        let mut project = Project::default();
        let rate = FrameRate::FPS_60;
        let comp = Comp::new(16, 16, rate, rate.tick_of(2));
        let comp_id = project.add_item("Transfer Test", Label::None, None, ItemKind::Comp(Arc::new(comp.clone())));
        let footage =
            Footage { path: source.to_string_lossy().into_owned(), kind: FootageKind::Still, width: 16, height: 16, has_video: true, ..Footage::default() };
        let footage_id = project.add_item("Source", Label::None, None, ItemKind::Footage(footage));
        let layer = effectcraft_project::build::layer(&mut project, &comp, "Source", LayerSource::Footage { item: footage_id }, (16, 16), Some(comp.duration));
        project.comp_mut(comp_id).unwrap().layers.push(layer);
        let project_path = dir.join("host.ecproj");
        std::fs::write(&project_path, project.to_file_json().unwrap()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let host = Config {
            project: project_path,
            cli: cli.clone(),
            output: dir.clone(),
            comp: "Transfer Test".into(),
            address: address.to_string(),
            token: "test-password".into(),
            gpu: false,
            range_start: 0,
            range_end: 2,
            chunk_frames: 2,
            max_workers: 1,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&host).unwrap();
        let state = Arc::new(Mutex::new(State {
            chunks: vec![Chunk { start: 0, end: 2, status: ChunkStatus::Pending, attempts: 0, worker: String::new() }],
            total: 2,
            max_workers: 1,
            dispatch_enabled: true,
            assembly_started: true,
            output: dir.clone(),
            ..State::default()
        }));
        let server_state = state.clone();
        let server = thread::spawn(move || {
            let (stream, peer) = listener.accept().unwrap();
            serve_worker(stream, peer, &host, &job, &server_state, &AtomicBool::new(false))
        });
        let client = Config {
            project: dir.join("missing-client.ecproj"),
            cli,
            output: dir.join("missing-shared-folder"),
            comp: "stale".into(),
            address: address.to_string(),
            token: "test-password".into(),
            gpu: false,
            range_start: 0,
            range_end: 0,
            chunk_frames: 1,
            max_workers: 1,
            final_video: dir.join("unused.mp4"),
            disabled_workers: Vec::new(),
        };
        assert!(run_worker(client, state.clone(), Arc::new(AtomicBool::new(false))).is_ok());
        assert!(server.join().unwrap().is_ok());
        assert!(chunk_complete(&dir, 0, 2));
        assert_eq!(worker_snapshot(&state).0.first().unwrap().completed_frames, 2);
        assert_eq!(lock(&state).local_progress, Some((0, 2, 2)));
        assert!(lock(&state).messages.iter().any(|message| message.contains("임시 프로젝트·소스·프레임 삭제 완료")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn host_caps_simultaneous_workers_and_assigns_next_client() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-multi-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut project = Project::default();
        project.add_item("Test", Label::None, None, ItemKind::Comp(Arc::new(Comp::new(2, 2, FrameRate::FPS_60, FrameRate::FPS_60.tick_of(4)))));
        let project_path = dir.join("source.ecproj");
        std::fs::write(&project_path, project.to_json()).unwrap();
        let config = Config {
            project: project_path,
            cli: std::env::current_exe().unwrap(),
            output: dir.clone(),
            comp: "Test".into(),
            address: "127.0.0.1:0".into(),
            token: "password".into(),
            gpu: false,
            range_start: 0,
            range_end: 4,
            chunk_frames: 2,
            max_workers: 1,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&config).unwrap();
        let state = Arc::new(Mutex::new(State {
            chunks: vec![
                Chunk { start: 0, end: 2, status: ChunkStatus::Pending, attempts: 0, worker: String::new() },
                Chunk { start: 2, end: 4, status: ChunkStatus::Pending, attempts: 0, worker: String::new() },
            ],
            total: 4,
            output: dir.clone(),
            max_workers: 1,
            dispatch_enabled: true,
            assembly_started: true,
            ..State::default()
        }));
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server_state = state.clone();
        let server = thread::spawn(move || {
            let mut handles = Vec::new();
            for index in 0..2 {
                let (stream, peer) = listener.accept().unwrap();
                let ip = if index == 0 { "127.0.0.1" } else { "127.0.0.2" }.parse().unwrap();
                let peer = SocketAddr::new(ip, peer.port());
                let config = config.clone();
                let job = job.clone();
                let state = server_state.clone();
                handles.push(thread::spawn(move || serve_worker(stream, peer, &config, &job, &state, &AtomicBool::new(false))));
            }
            handles.into_iter().all(|handle| handle.join().unwrap().is_ok())
        });
        let mut first = TcpStream::connect(address).unwrap();
        first.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut first_reader = BufReader::new(first.try_clone().unwrap());
        send(&mut first, &Message::Hello { token: "password".into(), name: "first".into(), protocol: PROTOCOL_VERSION }).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Ready { comp }) if comp == "Test"));
        send(&mut first, &Message::Request).unwrap();
        receive_test_bundle(&mut first_reader);
        send(&mut first, &Message::Request).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Assign { start: 0, end: 2, .. })));
        let mut second = TcpStream::connect(address).unwrap();
        second.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut second_reader = BufReader::new(second.try_clone().unwrap());
        send(&mut second, &Message::Hello { token: "password".into(), name: "second".into(), protocol: PROTOCOL_VERSION }).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Ready { comp }) if comp == "Test"));
        send(&mut second, &Message::Request).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Wait)));
        send_test_frames(&mut first, 0, 2);
        send(&mut first, &Message::Complete { start: 0, end: 2, ok: true, detail: String::new() }).unwrap();
        for _ in 0..100 {
            if lock(&state).chunks.first().is_some_and(|chunk| chunk.status == ChunkStatus::Done) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(lock(&state).chunks.first().is_some_and(|chunk| chunk.status == ChunkStatus::Done));
        send(&mut second, &Message::Request).unwrap();
        receive_test_bundle(&mut second_reader);
        send(&mut second, &Message::Request).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Assign { start: 2, end: 4, .. })));
        send(&mut first, &Message::Request).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Wait)));
        send_test_frames(&mut second, 2, 4);
        send(&mut second, &Message::Complete { start: 2, end: 4, ok: true, detail: String::new() }).unwrap();
        for _ in 0..100 {
            if lock(&state).chunks.get(1).is_some_and(|chunk| chunk.status == ChunkStatus::Done) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(lock(&state).chunks.get(1).is_some_and(|chunk| chunk.status == ChunkStatus::Done));
        send(&mut first, &Message::Request).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Finished)));
        send(&mut second, &Message::Request).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Finished)));
        assert!(server.join().unwrap());
        assert_eq!(snapshot(&state).0, 4);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn assembly_preserves_original_audio_and_uses_rendered_frames() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-assembly-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let audio_path = dir.join("silence.wav");
        let pcm = vec![0u8; 48_000 * 4];
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36_u32 + pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&48_000_u32.to_le_bytes());
        wav.extend_from_slice(&192_000_u32.to_le_bytes());
        wav.extend_from_slice(&4_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(pcm.len() as u32).to_le_bytes());
        wav.extend_from_slice(&pcm);
        std::fs::write(&audio_path, wav).unwrap();
        let mut project = Project::default();
        let rate = FrameRate::new(4, 1);
        let duration = Tick::from_seconds_f64(1.0);
        let comp_id = project.add_item("Test", Label::None, None, ItemKind::Comp(Arc::new(Comp::new(64, 64, rate, duration))));
        let audio = Footage { path: audio_path.to_string_lossy().into_owned(), kind: FootageKind::Audio, duration, has_audio: true, ..Footage::default() };
        let audio_id = project.add_item("Audio", Label::None, None, ItemKind::Footage(audio));
        let comp = project.comp(comp_id).unwrap().clone();
        let audio_layer = effectcraft_project::build::layer(&mut project, &comp, "Audio", LayerSource::Footage { item: audio_id }, (0, 0), Some(duration));
        project.comp_mut(comp_id).unwrap().layers.push(audio_layer);
        let project_path = dir.join("source.ecproj");
        std::fs::write(&project_path, project.to_file_json().unwrap()).unwrap();
        for frame in 0..4 {
            let pixels = image::RgbaImage::from_pixel(64, 64, image::Rgba([frame as u8 * 40, 30, 80, 255]));
            pixels.save(frame_path(&dir, frame)).unwrap();
        }
        let cli = std::env::var_os("EFFECTCRAFT_FARM_TEST_CLI").map(PathBuf::from).unwrap_or_else(|| std::env::current_exe().unwrap());
        let config = Config {
            project: project_path,
            cli,
            output: dir.clone(),
            comp: "Test".into(),
            address: "127.0.0.1:0".into(),
            token: "password".into(),
            gpu: false,
            range_start: 1,
            range_end: 3,
            chunk_frames: 2,
            max_workers: 2,
            final_video: dir.join("final.mp4"),
            disabled_workers: Vec::new(),
        };
        let job = read_job(&config).unwrap();
        let assembled = assembly_project(&config, &job).unwrap();
        let assembled_comp = assembled.comp(comp_id).unwrap();
        assert_eq!(assembled_comp.layers.len(), 2);
        assert!(assembled_comp.layers.first().unwrap().switches.video);
        assert!(!assembled_comp.layers.first().unwrap().switches.audio);
        assert_eq!(assembled_comp.layers.first().unwrap().start_time, rate.tick_of(1));
        assert_eq!(assembled_comp.layers.first().unwrap().out_point, rate.tick_of(3));
        assert!(!assembled_comp.layers.get(1).unwrap().switches.video);
        assert!(assembled_comp.layers.get(1).unwrap().switches.audio);
        if std::env::var_os("EFFECTCRAFT_FARM_TEST_CLI").is_some() {
            assemble_final(&config, &job).unwrap();
            assert!(std::fs::metadata(&config.final_video).unwrap().len() > 128);
        }
        if std::env::var_os("EFFECTCRAFT_FARM_KEEP_TEST_OUTPUT").is_some() {
            eprintln!("test output: {}", dir.display());
        } else {
            std::fs::remove_dir_all(dir).unwrap();
        }
    }
}
