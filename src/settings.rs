use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Distribution {
    #[default]
    Auto,
    Frames,
    Percent,
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RenderOptions {
    pub automatic: bool,
    pub nvenc: bool,
    pub ffmpeg: String,
    pub adaptive: bool,
    pub target_seconds: u32,
    pub retries: u8,
    pub previews: bool,
    pub preview_seconds: u32,
    pub poll_ms: u64,
    pub draft: bool,
    pub video_bitrate: u32,
    pub audio: bool,
    pub durable_writes: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            automatic: true,
            nvenc: true,
            ffmpeg: String::new(),
            adaptive: true,
            target_seconds: 30,
            retries: 3,
            previews: true,
            preview_seconds: 3,
            poll_ms: 150,
            draft: false,
            video_bitrate: 16000,
            audio: true,
            durable_writes: false,
        }
    }
}

impl RenderOptions {
    pub fn validate(&self) -> Result<(), String> {
        if !(5..=300).contains(&self.target_seconds)
            || !(1..=10).contains(&self.retries)
            || !(1..=30).contains(&self.preview_seconds)
            || !(50..=2000).contains(&self.poll_ms)
            || !(100..=200_000).contains(&self.video_bitrate)
        {
            return Err("렌더 옵션 범위를 확인하세요".into());
        }
        Ok(())
    }
}

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub queue: Vec<crate::queue::Task>,
    pub queue_cards: bool,
    pub reduced_motion: bool,
    #[serde(default)]
    pub gpu_defaults_applied: bool,
    pub check_updates: bool,
    pub revision: u64,
    pub project: String,
    pub cli: String,
    pub comp: String,
    pub host_address: String,
    pub client_address: String,
    pub token: String,
    pub gpu: bool,
    pub host: bool,
    pub local_worker: bool,
    pub range_start: i64,
    pub range_end: i64,
    pub range_comp: String,
    pub chunk_frames: i64,
    pub distribution: Distribution,
    pub chunk_percent: f64,
    pub max_workers: usize,
    pub final_video: String,
    pub disabled_workers: Vec<String>,
    pub render: RenderOptions,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            queue: Vec::new(),
            queue_cards: false,
            gpu_defaults_applied: true,
            reduced_motion: false,
            check_updates: true,
            revision: 0,
            project: String::new(),
            cli: String::new(),
            comp: String::new(),
            host_address: "0.0.0.0:9778".into(),
            client_address: String::new(),
            token: String::new(),
            gpu: true,
            host: true,
            local_worker: true,
            range_start: 0,
            range_end: 0,
            range_comp: String::new(),
            chunk_frames: 300,
            distribution: Distribution::Auto,
            chunk_percent: 50.0,
            max_workers: 2,
            final_video: String::new(),
            disabled_workers: Vec::new(),
            render: RenderOptions::default(),
        }
    }
}

impl Settings {
    pub fn frame_output(&self) -> PathBuf {
        frame_output(&self.project)
    }

    pub fn effective_chunk_frames(&self, total_frames: i64) -> Result<i64, String> {
        let end = if self.range_end == 0 { total_frames } else { self.range_end };
        if !(1..=100_000).contains(&total_frames) || self.range_start < 0 || end <= self.range_start || end > total_frames {
            return Err("선택한 컴포지션의 렌더 프레임 범위를 확인하세요".into());
        }
        let span = end - self.range_start;
        let frames = match self.distribution {
            Distribution::Auto => {
                if !(1..=64).contains(&self.max_workers) {
                    return Err("사용할 PC 수는 1~64대여야 합니다".into());
                }
                // Leave work in the shared queue so faster machines can help.
                ((span + self.max_workers as i64 * 8 - 1) / (self.max_workers as i64 * 8)).clamp(1, 120)
            }
            Distribution::Frames => self.chunk_frames,
            Distribution::Percent => {
                if !self.chunk_percent.is_finite() || !(0.01..=100.0).contains(&self.chunk_percent) {
                    return Err("구간 비율은 0.01~100%여야 합니다".into());
                }
                (span as f64 * self.chunk_percent / 100.0).ceil() as i64
            }
        };
        if !(1..=100_000).contains(&frames) {
            return Err("구간 크기는 1~100,000프레임이어야 합니다".into());
        }
        Ok(frames)
    }
}

/// Derive the host frame directory from the selected project, never from an MP4 or saved setting.
pub fn frame_output(project: &str) -> PathBuf {
    PathBuf::from(project).parent().unwrap_or_else(|| std::path::Path::new("")).join("frame-share")
}

fn directory() -> Result<PathBuf, String> {
    #[cfg(windows)]
    let base = std::env::var_os("APPDATA").map(PathBuf::from);
    #[cfg(target_os = "macos")]
    let base = std::env::var_os("HOME").map(|p| PathBuf::from(p).join("Library/Application Support"));
    #[cfg(all(not(windows), not(target_os = "macos")))]
    let base = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".config")));
    base.map(|p| p.join("EffectCraft").join("render-farm")).ok_or("사용자 설정 폴더를 찾을 수 없습니다".into())
}

pub fn log_path() -> Result<PathBuf, String> {
    Ok(directory()?.join("render-farm.log"))
}

pub fn update_result_path() -> Result<PathBuf, String> {
    Ok(directory()?.join("update-result.txt"))
}

fn slots(dir: &std::path::Path) -> [PathBuf; 2] {
    [dir.join("settings-a.json"), dir.join("settings-b.json")]
}

fn read_slot(path: &std::path::Path) -> Result<Option<(Settings, std::time::SystemTime)>, String> {
    if !path.exists() {
        return Ok(None);
    }
    let metadata = std::fs::metadata(path).map_err(|e| format!("설정 정보 읽기: {e}"))?;
    if metadata.len() > 4_000_000 {
        return Err(format!("설정 파일이 너무 큽니다: {}", path.display()));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("설정 읽기: {e}"))?;
    let settings = serde_json::from_slice(&bytes).map_err(|e| format!("설정 파일 형식 오류 ({}): {e}", path.display()))?;
    let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
    Ok(Some((settings, modified)))
}

pub fn load() -> (Settings, Option<String>) {
    let Ok(dir) = directory() else {
        return (Settings::default(), Some("사용자 설정 폴더를 찾을 수 없습니다".into()));
    };
    load_from(&dir)
}

fn load_from(dir: &std::path::Path) -> (Settings, Option<String>) {
    let mut valid = Vec::new();
    let mut errors = Vec::new();
    for path in slots(dir) {
        match read_slot(&path) {
            Ok(Some(value)) => valid.push(value),
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    valid.sort_by_key(|(settings, modified)| (settings.revision, *modified));
    let mut settings = valid.pop().map(|(settings, _)| settings).unwrap_or_default();
    if !settings.gpu_defaults_applied {
        settings.gpu = true;
        settings.gpu_defaults_applied = true;
    }
    (settings, (!errors.is_empty()).then(|| errors.join("; ")))
}

pub fn save(settings: &mut Settings) -> Result<(), String> {
    let dir = directory()?;
    save_to(&dir, settings)
}

fn save_to(dir: &std::path::Path, settings: &mut Settings) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("설정 폴더 만들기: {e}"))?;
    let [a, b] = slots(dir);
    let active = match (read_slot(&a).ok().flatten(), read_slot(&b).ok().flatten()) {
        (Some((sa, ta)), Some((sb, tb))) => {
            if (sa.revision, ta) >= (sb.revision, tb) {
                Some(a.clone())
            } else {
                Some(b.clone())
            }
        }
        (Some(_), None) => Some(a.clone()),
        (None, Some(_)) => Some(b.clone()),
        (None, None) => None,
    };
    settings.revision = settings.revision.saturating_add(1);
    let target = if active.as_ref() == Some(&a) { b } else { a };
    let temporary = target.with_extension(format!("tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(settings).map_err(|e| format!("설정 인코딩: {e}"))?;
    {
        let mut file = std::fs::File::create(&temporary).map_err(|e| format!("설정 임시 파일 만들기: {e}"))?;
        std::io::Write::write_all(&mut file, &bytes).map_err(|e| format!("설정 쓰기: {e}"))?;
        file.sync_all().map_err(|e| format!("설정 저장: {e}"))?;
    }
    if target.exists() {
        std::fs::remove_file(&target).map_err(|e| format!("이전 설정 교체: {e}"))?;
    }
    std::fs::rename(&temporary, &target).map_err(|e| format!("설정 확정: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enables_gpu_on_upgrade_once_then_preserves_manual_cpu_choice() {
        assert!(Settings::default().gpu);
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("farm-gpu-settings-{}-{suffix}", std::process::id()));
        std::fs::create_dir(&dir).unwrap();
        let mut legacy = serde_json::to_value(Settings::default()).unwrap();
        legacy.as_object_mut().unwrap().remove("gpu_defaults_applied");
        legacy["gpu"] = false.into();
        legacy["render"].as_object_mut().unwrap().remove("nvenc");
        legacy["render"].as_object_mut().unwrap().remove("automatic");
        std::fs::write(dir.join("settings-a.json"), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let (mut settings, warning) = load_from(&dir);
        assert!(warning.is_none());
        assert!(settings.gpu);
        assert!(settings.gpu_defaults_applied);
        assert!(settings.render.nvenc);
        assert!(settings.render.automatic);
        settings.gpu = false;
        settings.render.automatic = false;
        save_to(&dir, &mut settings).unwrap();
        assert!(!load_from(&dir).0.gpu);
        assert!(!load_from(&dir).0.render.automatic);
        assert!(dir.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn distribution_supports_auto_frames_and_percent() {
        let mut settings = Settings { max_workers: 3, range_start: 100, range_end: 1000, ..Settings::default() };
        assert_eq!(settings.effective_chunk_frames(1200).unwrap(), 38);
        settings.distribution = Distribution::Frames;
        settings.chunk_frames = 240;
        assert_eq!(settings.effective_chunk_frames(1200).unwrap(), 240);
        settings.distribution = Distribution::Percent;
        settings.chunk_percent = 25.0;
        assert_eq!(settings.effective_chunk_frames(1200).unwrap(), 225);
        settings.chunk_percent = f64::NAN;
        assert!(settings.effective_chunk_frames(1200).is_err());
    }

    #[test]
    fn settings_roundtrip_preserves_connection_and_distribution() {
        let value = Settings {
            client_address: "192.168.1.7:9778".into(),
            range_start: 120,
            range_end: 960,
            range_comp: "Test".into(),
            max_workers: 5,
            disabled_workers: vec!["192.168.1.8".into()],
            ..Settings::default()
        };
        let bytes = serde_json::to_vec(&value).unwrap();
        let restored: Settings = serde_json::from_slice(&bytes).unwrap();
        assert!(value == restored);
    }

    #[test]
    fn saved_settings_survive_restart_and_corrupt_latest_slot() {
        let suffix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("effectcraft-farm-settings-{}-{suffix}", std::process::id()));
        let mut value = Settings { client_address: "192.168.1.7:9778".into(), token: "test-secret".into(), ..Settings::default() };
        save_to(&dir, &mut value).unwrap();
        value.client_address = "192.168.1.9:9778".into();
        save_to(&dir, &mut value).unwrap();
        let (restored, warning) = load_from(&dir);
        assert_eq!(restored.client_address, "192.168.1.9:9778");
        assert_eq!(restored.token, "test-secret");
        assert!(warning.is_none());
        let latest =
            if read_slot(&dir.join("settings-a.json")).unwrap().unwrap().0.revision > read_slot(&dir.join("settings-b.json")).unwrap().unwrap().0.revision {
                dir.join("settings-a.json")
            } else {
                dir.join("settings-b.json")
            };
        std::fs::write(latest, b"broken").unwrap();
        let (fallback, warning) = load_from(&dir);
        assert_eq!(fallback.client_address, "192.168.1.7:9778");
        assert!(warning.is_some());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
