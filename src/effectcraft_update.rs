//! EffectCraft engine compatibility and its official installer, separate from Farm updates.
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use crate::updater::{self, Asset, Release};

const API: &str = "https://api.github.com/repos/storytold/effectcraft/releases";
const REPO: &str = "https://github.com/storytold/effectcraft/";
const MAX_INSTALLER: u64 = 512 * 1024 * 1024;

pub fn version(value: &str) -> Result<semver::Version, String> {
    if value.len() > 100 {
        return Err("EffectCraft 버전 문자열이 너무 깁니다".into());
    }
    semver::Version::parse(value.strip_prefix('v').unwrap_or(value)).map_err(|e| format!("EffectCraft 버전 형식: {e}"))
}

pub fn higher(left: &str, right: &str) -> Result<bool, String> {
    Ok(version(left)?.cmp_precedence(&version(right)?).is_gt())
}

pub fn latest_version(left: Option<&str>, right: Option<&str>) -> Result<Option<String>, String> {
    match (left, right) {
        (Some(a), Some(b)) => Ok(Some(if higher(a, b)? { a } else { b }.into())),
        (Some(value), None) | (None, Some(value)) => Ok(Some(version(value)?.to_string())),
        (None, None) => Ok(None),
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Requirement {
    pub local: String,
    pub peer: Option<String>,
    pub latest: Option<String>,
    pub target: String,
}

/// Each computer receives a requirement only if its own engine needs upgrading.
pub fn requirement(local: &str, peer: Option<&str>, latest: Option<&str>) -> Result<Option<Requirement>, String> {
    let mut target = version(local)?;
    for candidate in [peer, latest].into_iter().flatten() {
        let candidate = version(candidate)?;
        if candidate.cmp_precedence(&target).is_gt() {
            target = candidate;
        }
    }
    if !target.cmp_precedence(&version(local)?).is_gt() {
        return Ok(None);
    }
    Ok(Some(Requirement { local: local.into(), peer: peer.map(str::to_owned), latest: latest.map(str::to_owned), target: target.to_string() }))
}

pub fn installed_version(cli: &Path) -> Result<String, String> {
    let mut command = std::process::Command::new(cli);
    command.arg("--version").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().map_err(|e| format!("EffectCraft 버전 확인 실행: {e}"))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if child.try_wait().map_err(|e| e.to_string())?.is_some() {
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err("EffectCraft CLI 버전 확인에 실패했습니다. 설치를 복구하세요.".into());
            }
            let text = String::from_utf8_lossy(&output.stdout);
            let number = text.trim().strip_prefix("effectcraft-cli ").ok_or("EffectCraft CLI 버전 응답이 올바르지 않습니다")?;
            return Ok(version(number)?.to_string());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("EffectCraft 버전 확인 시간이 초과됐습니다".into());
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn fetch(tag: Option<&str>) -> Result<Release, String> {
    let endpoint = match tag {
        Some(tag) => format!("{API}/tags/v{}", version(tag)?),
        None => format!("{API}/latest"),
    };
    let release = updater::fetch_release(&endpoint, REPO)?;
    let number = version(&release.tag_name)?;
    if release.draft || (tag.is_none() && (release.prerelease || !number.pre.is_empty())) {
        return Err("공식 정식 EffectCraft 릴리스가 아닙니다".into());
    }
    if let Some(tag) = tag
        && number.cmp_precedence(&version(tag)?).is_ne()
    {
        return Err("요청한 EffectCraft 버전과 릴리스가 다릅니다".into());
    }
    Ok(release)
}

fn windows_asset<'a>(release: &'a Release, arch: &str) -> Result<&'a Asset, String> {
    if !matches!(arch, "x64" | "arm64") {
        return Err("자동 설치를 지원하지 않는 Windows 아키텍처입니다".into());
    }
    let number = version(&release.tag_name)?;
    let expected = format!("effectcraft-{number}-windows-{arch}.msi");
    let expected_url = format!("{REPO}releases/download/{}/{expected}", release.tag_name);
    let asset =
        release.assets.iter().find(|asset| asset.name == expected).ok_or("이 버전의 공식 Windows 설치 프로그램이 없습니다. 릴리스 페이지를 확인하세요.")?;
    if asset.size == 0 || asset.size > MAX_INSTALLER || asset.browser_download_url != expected_url {
        return Err("EffectCraft 설치 프로그램의 출처 또는 크기가 올바르지 않습니다".into());
    }
    updater::expected_digest(asset)?;
    Ok(asset)
}

fn native_arch() -> &'static str {
    // An x64 Farm can run under emulation on Windows ARM64.
    if cfg!(target_arch = "aarch64")
        || std::env::var("PROCESSOR_ARCHITEW6432").or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE")).is_ok_and(|value| value.eq_ignore_ascii_case("ARM64"))
    {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "x64"
    } else {
        "unsupported"
    }
}

struct Stage(PathBuf);

impl Drop for Stage {
    fn drop(&mut self) {
        // Created here under the OS temp directory; never an installation or project path.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn install(target: &str, cli: &Path, progress: &AtomicU64) -> Result<(PathBuf, String), String> {
    if !cfg!(windows) {
        return Err("자동 설치는 Windows에서 지원합니다. 공식 릴리스 페이지를 이용하세요.".into());
    }
    let current = installed_version(cli)?;
    if !higher(target, &current)? {
        return Ok((cli.to_path_buf(), current));
    }
    // Never execute a URL or file supplied by a LAN peer. Resolve the exact tag on official GitHub.
    let release = fetch(Some(target))?;
    let asset = windows_asset(&release, native_arch())?;
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let directory = std::env::temp_dir().join(format!("effectcraft-engine-update-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&directory).map_err(|e| format!("EffectCraft 업데이트 임시 폴더: {e}"))?;
    let stage = Stage(directory);
    let installer = stage.0.join(&asset.name);
    updater::download_installer(asset, &installer, progress)?;
    let current = installed_version(cli)?;
    if !higher(target, &current)? {
        return Ok((cli.to_path_buf(), current));
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let msiexec = PathBuf::from(std::env::var_os("SystemRoot").ok_or("SystemRoot가 없습니다")?).join("System32/msiexec.exe");
        // Keep Farm alive and locked until the visible official installer finishes.
        let status = std::process::Command::new(msiexec)
            .arg("/i")
            .arg(&installer)
            .arg("/norestart")
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| format!("EffectCraft 설치 프로그램 실행: {e}"))?;
        match status.code() {
            Some(0) => {}
            Some(1602) => return Err("EffectCraft 설치를 취소했습니다. 다시 업데이트할 수 있습니다.".into()),
            Some(3010) => return Err("EffectCraft 설치가 완료됐습니다. Windows를 재시작한 뒤 Farm을 실행하세요.".into()),
            code => return Err(format!("EffectCraft 설치 실패 (Windows Installer: {code:?})")),
        }
    }
    let installation = crate::installation::detect(&cli.to_string_lossy())?;
    let actual = installed_version(&installation.cli)?;
    if higher(target, &actual)? {
        return Err(format!("설치 후 EffectCraft가 {actual}입니다. 필요한 버전 {target}의 설치를 완료하세요."));
    }
    Ok((installation.cli, actual))
}

#[derive(Default)]
pub struct Updater {
    pub local: Option<String>,
    pub latest: Option<Release>,
    pub required: Option<Requirement>,
    pub prompt: bool,
    pub status: String,
    pub progress: Arc<AtomicU64>,
    probing: Option<Receiver<Result<String, String>>>,
    checking: Option<Receiver<Result<Release, String>>>,
    installing: Option<Receiver<Result<(PathBuf, String), String>>>,
    shown_target: Option<String>,
    probe_error: Option<String>,
}

impl Updater {
    pub fn checking(&self) -> bool {
        self.probing.is_some() || self.checking.is_some()
    }

    pub fn installing(&self) -> bool {
        self.installing.is_some()
    }

    pub fn check(&mut self, cli: PathBuf) {
        if self.checking() || self.installing() {
            return;
        }
        self.status = "설치된 EffectCraft와 공식 GitHub 릴리스 확인 중…".into();
        self.probe_error = None;
        let (tx, rx) = mpsc::channel();
        self.probing = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(installed_version(&cli));
        });
        let (tx, rx) = mpsc::channel();
        self.checking = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(fetch(None));
        });
    }

    pub fn observe(&mut self, required: Option<Requirement>) {
        if let Some(required) = required {
            if self.required.as_ref().is_some_and(|old| higher(&old.target, &required.target).unwrap_or(false)) {
                return;
            }
            if self.shown_target.as_deref() != Some(&required.target) {
                self.prompt = true;
                self.shown_target = Some(required.target.clone());
            }
            self.required = Some(required);
        }
    }

    /// Only invoked by the recommended action in the local update dialog.
    pub fn consent_and_install(&mut self, cli: PathBuf) {
        if self.checking() || self.installing() {
            return;
        }
        let Some(required) = self.required.clone() else { return };
        self.status = "EffectCraft를 다운로드·검증한 뒤 설치 창을 엽니다. 설치 창에서 진행하세요.".into();
        self.progress.store(0, Ordering::Relaxed);
        let progress = self.progress.clone();
        let (tx, rx) = mpsc::channel();
        self.installing = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(install(&required.target, &cli, &progress));
        });
    }

    pub fn poll(&mut self) -> Option<PathBuf> {
        if let Some(result) = self.probing.as_ref().and_then(updater::poll_receiver) {
            self.probing = None;
            match result {
                Ok(local) => {
                    if self.required.as_ref().is_some_and(|required| !higher(&required.target, &local).unwrap_or(true)) {
                        self.required = None;
                        self.prompt = false;
                        self.shown_target = None;
                    }
                    self.local = Some(local);
                }
                Err(error) => {
                    self.local = None;
                    self.probe_error = Some(error.clone());
                    self.status = error;
                }
            }
        }
        if let Some(result) = self.checking.as_ref().and_then(updater::poll_receiver) {
            self.checking = None;
            match result {
                Ok(release) => {
                    self.status = format!("공식 EffectCraft 최신 버전 {}", release.tag_name);
                    self.latest = Some(release);
                }
                Err(error) => self.status = format!("공식 EffectCraft 릴리스 확인 실패: {error} · 연결 시 PC 간 버전은 비교합니다."),
            }
        }
        if let Some(local) = &self.local
            && let Ok(required) = requirement(local, None, self.latest.as_ref().map(|release| release.tag_name.as_str()))
        {
            self.observe(required);
        }
        if let Some(error) = &self.probe_error {
            self.status = format!("{error} · EffectCraft 설치를 복구하고 버전을 다시 확인하세요.");
        }
        if let Some(result) = self.installing.as_ref().and_then(updater::poll_receiver) {
            self.installing = None;
            match result {
                Ok((cli, local)) => {
                    self.status = format!("EffectCraft {local} 설치 완료 · 호스트에 다시 연결하세요.");
                    self.local = Some(local);
                    self.required = None;
                    self.prompt = false;
                    self.shown_target = None;
                    return Some(cli);
                }
                Err(error) => self.status = format!("EffectCraft 업데이트: {error}"),
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_older_computer_is_required_to_update_in_either_direction() {
        for (local, peer, needs_update) in [("0.22.0", "0.23.0", true), ("0.23.0", "0.22.0", false), ("0.23.0", "0.23.0", false)] {
            assert_eq!(requirement(local, Some(peer), None).unwrap().is_some(), needs_update);
        }
    }

    #[test]
    fn official_release_newer_than_both_requires_both_and_never_downgrades() {
        for (local, peer) in [("0.22.0", "0.23.0"), ("0.23.0", "0.22.0"), ("0.22.0", "0.22.0")] {
            assert_eq!(requirement(local, Some(peer), Some("v0.23.1")).unwrap().unwrap().target, "0.23.1");
        }
        assert_eq!(requirement("0.22.0", Some("0.24.0"), Some("0.23.1")).unwrap().unwrap().target, "0.24.0");
        assert!(requirement("0.24.0", Some("0.22.0"), Some("0.23.1")).unwrap().is_none());
        assert!(requirement("0.23.1+local", Some("v0.23.1+official"), None).unwrap().is_none());
        assert!(requirement("0.23.1-rc.1", Some("0.23.1"), None).unwrap().is_some());
        assert!(requirement("unknown", Some("0.23.1"), None).is_err());
    }

    fn release() -> Release {
        serde_json::from_value(serde_json::json!({
            "tag_name": "v0.23.1", "html_url": format!("{REPO}releases/tag/v0.23.1"), "draft": false, "prerelease": false,
            "assets": [{"name": "effectcraft-0.23.1-windows-x64.msi", "size": 64, "digest": format!("sha256:{}", "a".repeat(64)),
                "browser_download_url": format!("{REPO}releases/download/v0.23.1/effectcraft-0.23.1-windows-x64.msi")}]
        }))
        .unwrap()
    }

    #[test]
    fn installer_requires_exact_official_version_architecture_and_sha256() {
        let mut release = release();
        assert!(windows_asset(&release, "x64").is_ok());
        assert!(windows_asset(&release, "arm64").is_err());
        assert!(windows_asset(&release, "x86").is_err());
        release.assets[0].browser_download_url = format!("{REPO}releases/download/v0.22.0/effectcraft-0.23.1-windows-x64.msi");
        assert!(windows_asset(&release, "x64").is_err());
        release.assets[0].browser_download_url = format!("{REPO}releases/download/v0.23.1/effectcraft-0.23.1-windows-x64.msi");
        release.assets[0].digest = None;
        assert!(windows_asset(&release, "x64").is_err());
    }

    #[test]
    fn repeated_peers_and_dismissal_do_not_repeat_prompts_or_install() {
        let mut updater = Updater::default();
        let required = requirement("0.22.0", Some("0.23.0"), None).unwrap();
        updater.observe(required.clone());
        assert!(updater.prompt);
        updater.prompt = false;
        updater.observe(required);
        assert!(!updater.prompt);
        assert!(!updater.installing());
        updater.observe(requirement("0.22.0", None, Some("0.23.1")).unwrap());
        assert!(updater.prompt);
        assert_eq!(updater.required.unwrap().target, "0.23.1");
    }

    #[test]
    fn rechecking_after_manual_install_clears_requirement_without_launching_setup() {
        let mut updater = Updater::default();
        updater.observe(requirement("0.22.0", Some("0.23.1"), None).unwrap());
        let (tx, rx) = mpsc::channel();
        updater.probing = Some(rx);
        tx.send(Ok("0.24.0".into())).unwrap();
        assert!(updater.poll().is_none());
        assert_eq!(updater.local.as_deref(), Some("0.24.0"));
        assert!(updater.required.is_none());
        assert!(!updater.prompt);
        assert!(!updater.installing());
    }

    #[test]
    fn github_success_does_not_hide_a_local_version_detection_failure() {
        let mut updater = Updater::default();
        let (tx, rx) = mpsc::channel();
        updater.probing = Some(rx);
        tx.send(Err("broken CLI".into())).unwrap();
        let (tx, rx) = mpsc::channel();
        updater.checking = Some(rx);
        tx.send(Ok(release())).unwrap();
        updater.poll();
        assert!(updater.status.contains("broken CLI"));
        assert!(updater.local.is_none());
        assert!(updater.latest.is_some());
        assert!(!updater.installing());
    }

    #[test]
    fn live_official_release_when_requested() {
        if std::env::var_os("EFFECTCRAFT_FARM_TEST_UPDATES").is_some() {
            let latest = fetch(None).unwrap();
            windows_asset(&latest, "x64").unwrap();
            let tagged = fetch(Some(&latest.tag_name)).unwrap();
            assert_eq!(latest.tag_name, tagged.tag_name);
        }
    }
}
