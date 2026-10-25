use std::path::{Path, PathBuf};

pub const INSTALL_URL: &str = "https://github.com/storytold/effectcraft/releases/latest";

#[derive(Clone, Debug)]
pub struct Installation {
    pub root: PathBuf,
    pub cli: PathBuf,
}

fn from_candidate(candidate: &Path) -> Option<Installation> {
    let root = if candidate.is_dir() { candidate } else { candidate.parent()? };
    let app = root.join(format!("effectcraft{}", std::env::consts::EXE_SUFFIX));
    let cli = root.join(format!("effectcraft-cli{}", std::env::consts::EXE_SUFFIX));
    // An old Farm ZIP's standalone CLI is not an EffectCraft installation.
    if !app.is_file() || !cli.is_file() {
        return None;
    }
    Some(Installation { root: root.to_path_buf(), cli })
}

fn first_installation(candidates: impl IntoIterator<Item = PathBuf>) -> Option<Installation> {
    candidates.into_iter().find_map(|path| from_candidate(&path))
}

#[cfg(windows)]
fn registered_paths() -> Vec<PathBuf> {
    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};
    let mut paths = Vec::new();
    for hive in [HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE] {
        let root = RegKey::predef(hive);
        for view in [KEY_WOW64_64KEY, KEY_WOW64_32KEY] {
            if let Ok(key) = root.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\App Paths\effectcraft.exe", KEY_READ | view) {
                for name in ["", "Path"] {
                    if let Ok(value) = key.get_value::<String, _>(name) {
                        paths.push(PathBuf::from(value.trim().trim_matches('"')));
                    }
                }
            }
            if let Ok(uninstall) = root.open_subkey_with_flags(r"Software\Microsoft\Windows\CurrentVersion\Uninstall", KEY_READ | view) {
                for name in uninstall.enum_keys().flatten() {
                    if let Ok(key) = uninstall.open_subkey(name)
                        && key.get_value::<String, _>("DisplayName").is_ok_and(|name| name.eq_ignore_ascii_case("EffectCraft"))
                        && let Ok(path) = key.get_value::<String, _>("InstallLocation")
                    {
                        paths.push(PathBuf::from(path.trim().trim_matches('"')));
                    }
                }
            }
        }
    }
    paths
}

pub fn detect(saved_cli: &str) -> Result<Installation, String> {
    #[cfg(windows)]
    let mut candidates = registered_paths();
    #[cfg(not(windows))]
    let mut candidates = Vec::new();
    for variable in ["ProgramW6432", "ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(variable) {
            candidates.push(PathBuf::from(base).join("EffectCraft"));
        }
    }
    if let Some(base) = std::env::var_os("LOCALAPPDATA") {
        candidates.push(PathBuf::from(base).join("Programs/EffectCraft"));
    }
    if !saved_cli.is_empty() {
        candidates.push(PathBuf::from(saved_cli));
    }
    #[cfg(target_os = "macos")]
    candidates.push(PathBuf::from("/Applications/EffectCraft.app/Contents/MacOS"));
    if let Some(paths) = std::env::var_os("PATH") {
        candidates.extend(std::env::split_paths(&paths));
    }
    first_installation(candidates).ok_or_else(|| {
        "EffectCraft 설치 경로에서 effectcraft와 effectcraft-cli를 찾을 수 없습니다. EffectCraft를 설치하거나 복구한 뒤 Render Farm을 다시 실행하세요.".into()
    })
}

pub fn guide_and_exit_reason(reason: &str) {
    let result = rfd::MessageDialog::new()
        .set_title("EffectCraft 설치 필요")
        .set_description(format!("{reason}\n\nEffectCraft 공식 설치 페이지를 열까요?\n이 안내를 닫으면 Render Farm은 종료됩니다."))
        .set_level(rfd::MessageLevel::Warning)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    if result == rfd::MessageDialogResult::Yes
        && let Err(error) = webbrowser::open(INSTALL_URL)
    {
        rfd::MessageDialog::new()
            .set_title("EffectCraft 설치 안내")
            .set_description(format!("설치 페이지를 열 수 없습니다: {error}\n\n브라우저에서 다음 주소를 여세요:\n{INSTALL_URL}"))
            .set_level(rfd::MessageLevel::Error)
            .show();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discovery_requires_both_effectcraft_and_its_cli_and_skips_stale_paths() {
        let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("effectcraft-detect-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let legacy = root.join("Farm ZIP");
        let installed = root.join("Custom EffectCraft 경로");
        std::fs::create_dir(&legacy).unwrap();
        std::fs::create_dir(&installed).unwrap();
        let cli_name = format!("effectcraft-cli{}", std::env::consts::EXE_SUFFIX);
        let app_name = format!("effectcraft{}", std::env::consts::EXE_SUFFIX);
        std::fs::write(legacy.join(&cli_name), b"old bundled CLI").unwrap();
        assert!(first_installation([legacy.clone()]).is_none());
        std::fs::write(installed.join(&app_name), b"EffectCraft").unwrap();
        assert!(from_candidate(&installed).is_none());
        std::fs::write(installed.join(&cli_name), b"installed CLI").unwrap();
        let result = first_installation([root.join("missing"), legacy, installed.join(app_name)]).unwrap();
        assert_eq!(result.root, installed);
        assert_eq!(result.cli, installed.join(cli_name));
        assert!(root.canonicalize().unwrap().starts_with(std::env::temp_dir().canonicalize().unwrap()));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installed_effectcraft_can_be_discovered_when_requested() {
        if std::env::var_os("EFFECTCRAFT_FARM_TEST_INSTALLED").is_some() {
            assert!(detect("").unwrap().cli.is_file());
        }
    }
}
