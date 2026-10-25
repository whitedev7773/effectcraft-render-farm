use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

use reqwest::blocking::Client;
use serde::Deserialize;
use sha2::{Digest, Sha256};

const API: &str = "https://api.github.com/repos/whitedev7773/effectcraft-render-farm/releases/latest";
const REPO: &str = "https://github.com/whitedev7773/effectcraft-render-farm/";
const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
const MAX_FILE: u64 = 512 * 1024 * 1024;

#[derive(Clone, Deserialize)]
pub struct Asset {
    name: String,
    size: u64,
    digest: Option<String>,
    browser_download_url: String,
}

#[derive(Clone, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub html_url: String,
    #[serde(default)]
    pub body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}

fn newer(release: &Release, current: &str) -> Result<bool, String> {
    let parse = |s: &str| semver::Version::parse(s.strip_prefix('v').unwrap_or(s)).map_err(|e| format!("버전 형식: {e}"));
    let candidate = parse(&release.tag_name)?;
    Ok(!release.draft && !release.prerelease && candidate.pre.is_empty() && candidate > parse(current)?)
}

fn client() -> Result<Client, String> {
    Client::builder()
        .user_agent(concat!("EffectCraft-Farm/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(900))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let trusted = attempt.url().scheme() == "https"
                && matches!(
                    attempt.url().host_str(),
                    Some("github.com" | "api.github.com" | "release-assets.githubusercontent.com" | "objects.githubusercontent.com")
                );
            if trusted && attempt.previous().len() < 5 { attempt.follow() } else { attempt.stop() }
        }))
        .build()
        .map_err(|e| format!("업데이트 HTTPS 설정: {e}"))
}

pub fn check() -> Result<Option<Release>, String> {
    let response = client()?
        .get(API)
        .timeout(Duration::from_secs(20))
        .header("Accept", "application/vnd.github+json")
        .send()
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("GitHub 릴리스 확인: {e}"))?;
    let mut bytes = Vec::new();
    response.take(1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 1024 * 1024 {
        return Err("릴리스 응답이 너무 큽니다".into());
    }
    let release: Release = serde_json::from_slice(&bytes).map_err(|e| format!("릴리스 응답: {e}"))?;
    if !release.html_url.starts_with(&format!("{REPO}releases/tag/")) {
        return Err("릴리스 주소가 저장소와 다릅니다".into());
    }
    Ok(newer(&release, env!("CARGO_PKG_VERSION"))?.then_some(release))
}

fn windows_asset(release: &Release) -> Result<&Asset, String> {
    let expected = format!("effectcraft-render-farm-{}-windows-x64.zip", release.tag_name);
    let asset = release.assets.iter().find(|a| a.name == expected).ok_or("이 릴리스에는 Windows x64 업데이트 ZIP이 없습니다")?;
    if asset.size == 0 || asset.size > MAX_ARCHIVE || !asset.browser_download_url.starts_with(&format!("{REPO}releases/download/")) {
        return Err("업데이트 파일 크기 또는 주소가 잘못됐습니다".into());
    }
    expected_digest(asset)?;
    Ok(asset)
}

fn expected_digest(asset: &Asset) -> Result<&str, String> {
    let digest = asset
        .digest
        .as_deref()
        .and_then(|s| s.strip_prefix("sha256:"))
        .ok_or("릴리스에 SHA-256이 없어 자동 설치할 수 없습니다. 릴리스 페이지에서 확인하세요")?;
    if digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("릴리스 SHA-256 형식 오류".into());
    }
    Ok(digest)
}

fn validate_pe(path: &Path) -> Result<(), String> {
    use std::io::{Seek, SeekFrom};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut header = [0; 64];
    file.read_exact(&mut header).map_err(|e| e.to_string())?;
    if &header[..2] != b"MZ" {
        return Err("업데이트 파일이 Windows 실행 파일이 아닙니다".into());
    }
    let offset = u32::from_le_bytes([header[60], header[61], header[62], header[63]]);
    file.seek(SeekFrom::Start(offset as u64)).map_err(|e| e.to_string())?;
    let mut pe = [0; 6];
    file.read_exact(&mut pe).map_err(|e| e.to_string())?;
    if &pe[..4] != b"PE\0\0" || pe[4..] != [0x64, 0x86] {
        return Err("업데이트 실행 파일이 Windows x64가 아닙니다".into());
    }
    Ok(())
}

fn extract(archive: &Path, stage: &Path) -> Result<(), String> {
    let file = std::fs::File::open(archive).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("업데이트 ZIP: {e}"))?;
    if zip.len() > 1000 {
        return Err("업데이트 ZIP 항목이 너무 많습니다".into());
    }
    let mut found = std::collections::BTreeSet::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let path = entry.enclosed_name().ok_or("업데이트 ZIP 경로 오류")?;
        if entry.is_dir() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        if !matches!(name, "effectcraft-farm.exe" | "effectcraft-cli.exe") {
            continue;
        }
        if entry.is_symlink() || entry.size() > MAX_FILE || !found.insert(name.to_owned()) {
            return Err("업데이트 ZIP 실행 파일 중복·크기·링크 오류".into());
        }
        let target = stage.join(name);
        let mut output = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(|e| e.to_string())?;
        let declared_size = entry.size();
        let copied = std::io::copy(&mut (&mut entry).take(MAX_FILE + 1), &mut output).map_err(|e| format!("업데이트 압축 해제: {e}"))?;
        if copied > MAX_FILE || copied != declared_size {
            return Err("업데이트 파일 크기 오류".into());
        }
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        validate_pe(&target)?;
    }
    if found.len() != 2 {
        return Err("ZIP에 앱과 CLI 실행 파일이 모두 필요합니다".into());
    }
    Ok(())
}

pub struct Prepared {
    stage: PathBuf,
    exe: PathBuf,
}

fn prepare(release: Release, progress: Arc<AtomicU64>) -> Result<Prepared, String> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return Err("자동 설치는 Windows x64에서 지원합니다".into());
    }
    let asset = windows_asset(&release)?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case("effectcraft-cli.exe")) {
        return Err("앱 실행 파일 이름을 변경하세요".into());
    }
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos();
    let stage = std::env::temp_dir().join(format!("effectcraft-farm-update-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&stage).map_err(|e| format!("업데이트 임시 폴더: {e}"))?;
    // Check write permission before asking the running app to exit.
    let install = exe.parent().ok_or("앱 설치 폴더가 없습니다")?;
    let probe = install.join(format!(".farm-update-write-{nonce}"));
    std::fs::OpenOptions::new().write(true).create_new(true).open(&probe).map_err(|e| format!("설치 폴더 쓰기 권한이 없습니다: {e}"))?;
    std::fs::remove_file(probe).map_err(|e| e.to_string())?;
    let mut response = client()?.get(&asset.browser_download_url).send().and_then(|r| r.error_for_status()).map_err(|e| format!("업데이트 다운로드: {e}"))?;
    let archive = stage.join("release.zip");
    let mut output = std::fs::File::create(&archive).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0; 256 * 1024];
    let mut downloaded = 0;
    loop {
        let count = response.read(&mut buffer).map_err(|e| format!("업데이트 수신: {e}"))?;
        if count == 0 {
            break;
        }
        downloaded += count as u64;
        if downloaded > asset.size || downloaded > MAX_ARCHIVE {
            return Err("업데이트 다운로드 크기 초과".into());
        }
        hash.update(&buffer[..count]);
        output.write_all(&buffer[..count]).map_err(|e| e.to_string())?;
        progress.store(downloaded, Ordering::Relaxed);
    }
    output.sync_all().map_err(|e| e.to_string())?;
    drop(output);
    if downloaded != asset.size || !format!("{:x}", hash.finalize()).eq_ignore_ascii_case(expected_digest(asset)?) {
        return Err("업데이트 크기 또는 SHA-256 검증 실패 · 설치하지 않았습니다".into());
    }
    extract(&archive, &stage)?;
    Ok(Prepared { stage, exe })
}

impl Prepared {
    pub fn launch(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let script = self.stage.join("install.ps1");
            std::fs::write(&script, include_str!("updater.ps1")).map_err(|e| e.to_string())?;
            let result = crate::settings::update_result_path()?;
            if let Some(parent) = result.parent() {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let powershell =
                std::env::var_os("SystemRoot").map(PathBuf::from).ok_or("SystemRoot가 없습니다")?.join("System32/WindowsPowerShell/v1.0/powershell.exe");
            let mut child = std::process::Command::new(powershell)
                .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script)
                .arg("-ParentId")
                .arg(std::process::id().to_string())
                .arg("-Stage")
                .arg(&self.stage)
                .arg("-Executable")
                .arg(&self.exe)
                .arg("-ResultFile")
                .arg(result)
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .creation_flags(0x0800_0000)
                .spawn()
                .map_err(|e| format!("업데이트 설치 도우미 실행: {e}"))?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                    return Err("업데이트 설치 도우미가 준비 중 종료됐습니다".into());
                }
                if self.stage.join("installer-ready").is_file() {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("업데이트 설치 도우미 준비 시간이 초과됐습니다".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        #[cfg(not(windows))]
        {
            Err("자동 설치는 Windows에서 지원합니다".into())
        }
    }
}

#[derive(Default)]
pub struct Updater {
    pub available: Option<Release>,
    pub prompt: bool,
    pub status: String,
    pub restarting: bool,
    pub progress: Arc<AtomicU64>,
    checking: Option<Receiver<Result<Option<Release>, String>>>,
    downloading: Option<Receiver<Result<Prepared, String>>>,
    checked: bool,
}

impl Updater {
    pub fn busy(&self) -> bool {
        self.checking.is_some() || self.installing()
    }
    pub fn installing(&self) -> bool {
        self.downloading.is_some() || self.restarting
    }
    pub fn check(&mut self) {
        if self.busy() {
            return;
        }
        self.checked = true;
        self.status = "GitHub 최신 버전 확인 중…".into();
        let (tx, rx) = mpsc::channel();
        self.checking = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(check());
        });
    }
    // Call only from the explicit consent button in the update dialog.
    pub fn consent_and_download(&mut self) {
        if self.busy() {
            return;
        }
        let Some(release) = self.available.clone() else { return };
        self.prompt = false;
        self.status = "동의한 업데이트를 다운로드·검증 중…".into();
        self.progress.store(0, Ordering::Relaxed);
        let progress = self.progress.clone();
        let (tx, rx) = mpsc::channel();
        self.downloading = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(prepare(release, progress));
        });
    }
    pub fn poll(&mut self, automatic: bool) -> Option<Result<Prepared, String>> {
        if automatic && !self.checked {
            self.check();
        }
        if let Some(result) = self.checking.as_ref().and_then(poll_receiver) {
            self.checking = None;
            match result {
                Ok(Some(release)) => {
                    self.status = format!("새 버전 {} 사용 가능", release.tag_name);
                    self.available = Some(release);
                    self.prompt = true;
                }
                Ok(None) => {
                    self.status = format!("현재 버전 {} · 새 버전 없음", env!("CARGO_PKG_VERSION"));
                    self.available = None;
                }
                Err(error) => self.status = error,
            }
        }
        let result = self.downloading.as_ref().and_then(poll_receiver);
        if result.is_some() {
            self.downloading = None;
        }
        result
    }
}

fn poll_receiver<T>(rx: &Receiver<Result<T, String>>) -> Option<Result<T, String>> {
    match rx.try_recv() {
        Ok(value) => Some(value),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Disconnected) => Some(Err("업데이트 작업이 중단됐습니다".into())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            html_url: format!("{REPO}releases/tag/{tag}"),
            body: None,
            draft: false,
            prerelease: false,
            assets: vec![Asset {
                name: format!("effectcraft-render-farm-{tag}-windows-x64.zip"),
                size: 100,
                digest: Some(format!("sha256:{}", "a".repeat(64))),
                browser_download_url: format!("{REPO}releases/download/{tag}/app.zip"),
            }],
        }
    }

    #[test]
    fn versions_never_downgrade_or_offer_preview_releases() {
        assert!(newer(&release("v0.10.0"), "0.6.0").unwrap());
        assert!(!newer(&release("v0.5.1"), "0.6.0").unwrap());
        assert!(!newer(&release("v0.6.0"), "0.6.0").unwrap());
        assert!(!newer(&release("v1.0.0-beta.1"), "0.6.0").unwrap());
        let mut draft = release("v1.0.0");
        draft.draft = true;
        assert!(!newer(&draft, "0.6.0").unwrap());
        assert!(newer(&release("latest"), "0.6.0").is_err());
    }

    #[test]
    fn asset_requires_expected_platform_origin_and_digest() {
        let mut value = release("v0.7.0");
        assert!(windows_asset(&value).is_ok());
        value.assets[0].digest = None;
        assert!(windows_asset(&value).is_err());
        value.assets[0].digest = Some(format!("sha256:{}", "a".repeat(64)));
        value.assets[0].browser_download_url = "https://example.com/app.zip".into();
        assert!(windows_asset(&value).is_err());
        value.assets[0].browser_download_url = format!("{REPO}releases/download/v0.7.0/app.zip");
        value.assets[0].name = "linux.zip".into();
        assert!(windows_asset(&value).is_err());
    }

    #[test]
    fn discovery_and_dismissal_do_not_download_or_install() {
        let mut updater = Updater { available: Some(release("v0.7.0")), prompt: true, ..Default::default() };
        assert!(updater.poll(false).is_none());
        assert!(!updater.installing());
        updater.prompt = false; // "나중에" or window close
        assert!(updater.poll(false).is_none());
        assert!(updater.available.is_some());
        assert!(!updater.installing());
    }

    fn write_archive(path: &Path, names: &[&str]) {
        let file = std::fs::File::create(path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let mut bytes = vec![0; 128];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&64u32.to_le_bytes());
        bytes[64..70].copy_from_slice(b"PE\0\0\x64\x86");
        for name in names {
            writer.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
            writer.write_all(&bytes).unwrap();
        }
        writer.finish().unwrap();
    }

    #[test]
    fn archive_extracts_only_pair_and_rejects_missing_duplicate_and_traversal() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("effectcraft-update-test-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        for (i, names, valid) in [
            (0, vec!["app/effectcraft-farm.exe", "app/effectcraft-cli.exe", "README.md"], true),
            (1, vec!["effectcraft-farm.exe"], false),
            (2, vec!["a/effectcraft-farm.exe", "b/effectcraft-farm.exe", "effectcraft-cli.exe"], false),
            (3, vec!["../effectcraft-farm.exe", "effectcraft-cli.exe"], false),
        ] {
            let stage = root.join(i.to_string());
            std::fs::create_dir(&stage).unwrap();
            let archive = stage.join("test.zip");
            write_archive(&archive, &names);
            assert_eq!(extract(&archive, &stage).is_ok(), valid);
            if valid {
                assert!(!stage.join("README.md").exists());
            }
        }
        // Only this uniquely created test directory is removed.
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_github_check_when_requested() {
        if std::env::var_os("EFFECTCRAFT_FARM_TEST_UPDATES").is_some() {
            check().unwrap();
        }
    }
}
