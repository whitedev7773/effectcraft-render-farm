use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
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
const PROTOCOL_VERSION: u32 = 2;
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
}

pub type Shared = Arc<Mutex<State>>;

impl State {
    pub fn note(&mut self, message: impl Into<String>) {
        self.messages.push(message.into());
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
        hash: String,
        name: String,
        #[serde(default)]
        protocol: u32,
    },
    Ready {
        comp: String,
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
        return Err(format!("공유 출력 폴더가 없습니다: {}", config.output.display()));
    }
    let start = config.range_start;
    let end = if config.range_end == 0 { total } else { config.range_end };
    if start < 0 || end <= start || end > total {
        return Err(format!("렌더 범위는 0 이상 {total} 이하이며 시작 < 끝이어야 합니다"));
    }
    preflight_resources(&project, **comp_id, &config.project)?;
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
            Some(CompInfo { name, frames, rate: comp.frame_rate, width: comp.width, height: comp.height })
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

fn project_file_hash(path: &Path) -> Result<String, String> {
    read_project_bytes(path).map(|bytes| project_hash(&bytes))
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
            return Err("공유 폴더에 다른 프로젝트의 렌더 결과가 있습니다".into());
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
            return Err("공유 폴더에 작업 정보 없는 기존 frame_ 파일이 있습니다".into());
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

fn serve_worker(mut stream: TcpStream, peer: SocketAddr, config: &Config, job: &Job, state: &Shared, stop: &AtomicBool) -> Result<(), String> {
    stream.set_read_timeout(Some(Duration::from_secs(3600))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let (name, hash, given_token, protocol) = match receive(&mut reader)? {
        Message::Hello { token, hash, name, protocol } => (name, hash, token, protocol),
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
    if given_token != config.token || hash != job.hash || name.len() > 80 {
        send(&mut stream, &Message::Error { detail: "접속 암호 또는 프로젝트가 올바르지 않습니다".into() })?;
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
            s.worker_info.insert(id, WorkerInfo { id, ip: ip.clone(), label: label.clone(), enabled, connected: true, assigned: None });
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
    let result = (|| -> Result<(), String> {
        loop {
            match receive(&mut reader) {
                Ok(Message::Request) if assigned.is_none() => {
                    if stop.load(Ordering::Relaxed) {
                        send(&mut stream, &Message::Finished)?;
                        break Ok(());
                    }
                    let next = {
                        let mut s = lock(state);
                        let finished = s.chunks.iter().all(|c| matches!(c.status, ChunkStatus::Done | ChunkStatus::Failed));
                        let enabled = s.dispatch_enabled && s.worker_info.get(&worker_id).is_some_and(|w| w.enabled);
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
                            }
                            Some(range)
                        } else {
                            None
                        }
                    };
                    match next {
                        Some((start, end)) => {
                            assigned = next;
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
                Ok(Message::Complete { start, end, ok, detail }) if assigned == Some((start, end)) => {
                    let verified = ok && chunk_complete(&job.output, start, end);
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
                            info.assigned = None;
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
    let hash = project_file_hash(&config.project)?;
    let mut stream = TcpStream::connect(&config.address).map_err(|e| format!("서버 연결: {e}"))?;
    stream.set_read_timeout(Some(Duration::from_secs(30))).map_err(|e| e.to_string())?;
    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);
    let name = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_else(|_| "worker".into());
    send(&mut stream, &Message::Hello { token: config.token.clone(), hash, name, protocol: PROTOCOL_VERSION })?;
    let comp = match receive(&mut reader)? {
        Message::Ready { comp } => comp,
        Message::Error { detail } => return Err(detail),
        _ => return Err("서버 인증 응답이 잘못됐습니다".into()),
    };
    let mut config = config;
    config.comp = comp;
    let job = match read_job(&config) {
        Ok(job) => job,
        Err(error) => {
            let _ = send(&mut stream, &Message::Error { detail: "클라이언트 리소스 검사 실패".into() });
            return Err(error);
        }
    };
    log(&state, format!("렌더 준비 완료: {}", job.comp));
    while !stop.load(Ordering::Relaxed) {
        send(&mut stream, &Message::Request)?;
        match receive(&mut reader)? {
            Message::Assign { start, end, fps_num, fps_den, comp } => {
                if start < 0 || end <= start || end > job.total || fps_num != job.rate.num || fps_den != job.rate.den || comp != job.comp {
                    return Err("서버 작업 범위 또는 설정이 프로젝트와 다릅니다".into());
                }
                log(&state, format!("렌더링: {start}–{end}"));
                let result = render_chunk(&config, &job, start, end);
                let (ok, detail) = match result {
                    Ok(()) => (true, "".into()),
                    Err(e) => (false, e),
                };
                send(&mut stream, &Message::Complete { start, end, ok, detail })?;
                log(&state, format!("구간 {start}–{end} {} · 다음 작업 대기", if ok { "완료" } else { "실패" }));
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
    Ok(())
}

fn render_chunk(config: &Config, job: &Job, start: i64, end: i64) -> Result<(), String> {
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
    let output = command.output().map_err(|e| format!("렌더 CLI 실행: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: String = stderr.chars().rev().take(500).collect::<String>().chars().rev().collect();
        return Err(format!("CLI 종료 {}: {tail}", output.status));
    }
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
    let (done, running, total, workers, failed, messages, output) = {
        let s = lock(state);
        let done = s.chunks.iter().filter(|c| c.status == ChunkStatus::Done).map(|c| c.end - c.start).sum::<i64>();
        let running = s.chunks.iter().filter(|c| c.status == ChunkStatus::Running).map(|c| (c.start, c.end)).collect::<Vec<_>>();
        let failed = s.chunks.iter().filter(|c| c.status == ChunkStatus::Failed).count();
        (done, running, s.total, s.workers, failed, s.messages.clone(), s.output.clone())
    };
    let in_progress = running.into_iter().map(|(start, end)| (start..end).filter(|frame| valid_png(&frame_path(&output, *frame))).count() as i64).sum::<i64>();
    (done.saturating_add(in_progress).min(total), total, workers, failed, messages, output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use effectcraft_color::Label;
    use effectcraft_project::{Comp, ItemKind};
    use effectcraft_time::Tick;

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
    fn project_validation_and_manifest_reject_mixed_jobs() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let project_path = dir.join("test.ecproj");
        let mut project = Project::default();
        let comp = Comp::new(64, 64, FrameRate::FPS_60, Tick::from_seconds_f64(2.0));
        project.add_item("Test", Label::None, None, ItemKind::Comp(Arc::new(comp)));
        std::fs::write(&project_path, project.to_json()).unwrap();
        let catalog = list_compositions(&project_path).unwrap();
        assert_eq!(catalog.len(), 1);
        assert_eq!(catalog[0].name, "Test");
        assert_eq!(catalog[0].frames, 120);
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
        let job = Job {
            hash: "same-project".into(),
            comp: "Test".into(),
            rate: FrameRate::FPS_60,
            total: 2,
            start: 0,
            end: 2,
            width: 2,
            height: 2,
            output: dir.clone(),
        };
        let config = Config {
            project: dir.join("test.ecproj"),
            cli: dir.join("cli.exe"),
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
        send(&mut client, &Message::Hello { token: "password".into(), hash: job.hash, name: "test-worker".into(), protocol: PROTOCOL_VERSION }).unwrap();
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
        assert!(matches!(receive(&mut reader), Ok(Message::Assign { start: 0, end: 2, .. })));
        for frame in 0..2 {
            let mut bytes = PNG_HEADER.to_vec();
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(b"IEND");
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            std::fs::write(frame_path(&dir, frame), bytes).unwrap();
        }
        send(&mut client, &Message::Complete { start: 0, end: 2, ok: true, detail: String::new() }).unwrap();
        send(&mut client, &Message::Request).unwrap();
        assert!(matches!(receive(&mut reader), Ok(Message::Finished)));
        assert!(server.join().unwrap().is_ok());
        assert_eq!(snapshot(&state).0, 2);
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
        project.add_item("Host Choice", Label::None, None, ItemKind::Comp(Arc::new(comp)));
        std::fs::write(&project_path, project.to_json()).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let mut config = Config {
            project: project_path,
            cli: std::env::current_exe().unwrap(),
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
        let state = Arc::new(Mutex::new(State::default()));
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
    fn host_caps_simultaneous_workers_and_assigns_next_client() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-multi-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let job =
            Job { hash: "same".into(), comp: "Test".into(), rate: FrameRate::FPS_60, total: 4, start: 0, end: 4, width: 2, height: 2, output: dir.clone() };
        let config = Config {
            project: dir.join("source.ecproj"),
            cli: dir.join("cli.exe"),
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
        send(&mut first, &Message::Hello { token: "password".into(), hash: "same".into(), name: "first".into(), protocol: PROTOCOL_VERSION }).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Ready { comp }) if comp == "Test"));
        send(&mut first, &Message::Request).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Assign { start: 0, end: 2, .. })));
        let mut second = TcpStream::connect(address).unwrap();
        second.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut second_reader = BufReader::new(second.try_clone().unwrap());
        send(&mut second, &Message::Hello { token: "password".into(), hash: "same".into(), name: "second".into(), protocol: PROTOCOL_VERSION }).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Ready { comp }) if comp == "Test"));
        send(&mut second, &Message::Request).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Wait)));
        for frame in 0..2 {
            let mut bytes = PNG_HEADER.to_vec();
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(b"IEND");
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            std::fs::write(frame_path(&dir, frame), bytes).unwrap();
        }
        send(&mut first, &Message::Complete { start: 0, end: 2, ok: true, detail: String::new() }).unwrap();
        for _ in 0..100 {
            if lock(&state).chunks.first().is_some_and(|chunk| chunk.status == ChunkStatus::Done) {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        assert!(lock(&state).chunks.first().is_some_and(|chunk| chunk.status == ChunkStatus::Done));
        send(&mut second, &Message::Request).unwrap();
        assert!(matches!(receive(&mut second_reader), Ok(Message::Assign { start: 2, end: 4, .. })));
        send(&mut first, &Message::Request).unwrap();
        assert!(matches!(receive(&mut first_reader), Ok(Message::Wait)));
        for frame in 2..4 {
            let mut bytes = PNG_HEADER.to_vec();
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            bytes.extend_from_slice(b"IEND");
            bytes.extend_from_slice(&[0, 0, 0, 0]);
            std::fs::write(frame_path(&dir, frame), bytes).unwrap();
        }
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
