use std::{
    env, fs,
    path::{Path, PathBuf},
    sync::RwLock,
};

use crate::{
    error::{Result, YoutubeAudioError},
    extractor::ytdlp::YtDlpExtractor,
    http::reload_youtube_cookie_header,
};

static COOKIE_PATH_OVERRIDE: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Set a custom override path for the YouTube cookies file.
pub fn set_cookie_path_override(path: Option<PathBuf>) {
    if let Ok(mut lock) = COOKIE_PATH_OVERRIDE.write() {
        *lock = path;
    }
}

/// Resolve the canonical path for `cookies.txt`.
pub fn resolve_cookie_path() -> PathBuf {
    if let Ok(lock) = COOKIE_PATH_OVERRIDE.read()
        && let Some(ref path) = *lock
    {
        return path.clone();
    }

    if let Ok(config_home) = env::var("XDG_CONFIG_HOME")
        && !config_home.is_empty()
    {
        return PathBuf::from(config_home).join("vortex-dl/cookies.txt");
    }

    let home = env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        return PathBuf::from(home).join(".config/vortex-dl/cookies.txt");
    }

    if let Ok(appdata) = env::var("APPDATA")
        && !appdata.is_empty()
    {
        return PathBuf::from(appdata).join("vortex-dl/cookies.txt");
    }

    PathBuf::from("./cookies.txt")
}

/// Check if a cookie file exists and contains authenticated YouTube session cookies (`LOGIN_INFO` or `SID`).
pub fn has_valid_youtube_session(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };

    let mut has_yt = false;
    let mut has_auth = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') || trimmed.is_empty() {
            continue;
        }

        let parts: Vec<&str> = trimmed.split('\t').collect();
        if parts.len() >= 7 && parts[0].contains("youtube.com") {
            has_yt = true;
            let name = parts[5];
            if name == "LOGIN_INFO" || name == "SID" || name == "SAPISID" {
                has_auth = true;
                break;
            }
        }
    }

    has_yt && has_auth
}

/// Check if a cookie file exists and has any YouTube cookies (even guest).
pub fn has_any_youtube_cookies(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    let Ok(content) = fs::read_to_string(path) else {
        return false;
    };

    content.lines().any(|l| {
        let trimmed = l.trim();
        !trimmed.starts_with('#') && trimmed.contains("youtube.com")
    })
}

/// Return installed browsers that can be probed for cookie extraction.
pub fn candidate_browsers() -> Vec<&'static str> {
    const ALL_BROWSERS: &[&str] = &[
        "firefox", "chrome", "chromium", "brave", "edge", "opera", "vivaldi",
    ];

    let installed: Vec<&'static str> = ALL_BROWSERS
        .iter()
        .copied()
        .filter(|&b| is_browser_installed(b))
        .collect();

    if installed.is_empty() {
        ALL_BROWSERS.to_vec()
    } else {
        installed
    }
}

fn is_browser_installed(browser: &str) -> bool {
    let home = env::var("HOME").unwrap_or_default();
    match browser {
        "firefox" => {
            Path::new(&format!("{home}/.mozilla/firefox")).exists()
                || Path::new(&format!("{home}/.librewolf")).exists()
                || Path::new(&format!("{home}/Library/Application Support/Firefox")).exists()
                || env::var("APPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Mozilla/Firefox")).exists())
        }
        "chrome" => {
            Path::new(&format!("{home}/.config/google-chrome")).exists()
                || Path::new(&format!("{home}/Library/Application Support/Google/Chrome")).exists()
                || env::var("LOCALAPPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Google/Chrome")).exists())
        }
        "chromium" => {
            Path::new(&format!("{home}/.config/chromium")).exists()
                || Path::new(&format!("{home}/Library/Application Support/Chromium")).exists()
                || env::var("LOCALAPPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Chromium")).exists())
        }
        "brave" => {
            Path::new(&format!("{home}/.config/BraveSoftware/Brave-Browser")).exists()
                || Path::new(&format!(
                    "{home}/Library/Application Support/BraveSoftware/Brave-Browser"
                ))
                .exists()
                || env::var("LOCALAPPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/BraveSoftware/Brave-Browser")).exists())
        }
        "edge" => {
            Path::new(&format!("{home}/.config/microsoft-edge")).exists()
                || Path::new(&format!(
                    "{home}/Library/Application Support/Microsoft Edge"
                ))
                .exists()
                || env::var("LOCALAPPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Microsoft/Edge")).exists())
        }
        "opera" => {
            Path::new(&format!("{home}/.config/opera")).exists()
                || Path::new(&format!(
                    "{home}/Library/Application Support/com.operasoftware.Opera"
                ))
                .exists()
                || env::var("APPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Opera Software/Opera Stable")).exists())
        }
        "vivaldi" => {
            Path::new(&format!("{home}/.config/vivaldi")).exists()
                || Path::new(&format!("{home}/Library/Application Support/Vivaldi")).exists()
                || env::var("LOCALAPPDATA")
                    .is_ok_and(|a| Path::new(&format!("{a}/Vivaldi")).exists())
        }
        _ => false,
    }
}

/// Automatically extract and import YouTube cookies from installed browsers into `~/.config/vortex-dl/cookies.txt`.
/// If `force_refresh` is false and a valid authenticated session is already present, this is a no-op.
pub async fn auto_import_cookies(force_refresh: bool) -> Result<PathBuf> {
    let target_path = resolve_cookie_path();

    if !force_refresh {
        if Path::new("./cookies.txt").exists()
            && has_valid_youtube_session(Path::new("./cookies.txt"))
        {
            return Ok(PathBuf::from("./cookies.txt"));
        }
        if target_path.exists() && has_valid_youtube_session(&target_path) {
            return Ok(target_path);
        }
    }

    let ytdlp_cmd = YtDlpExtractor::find_cmd().await?;
    let browsers = candidate_browsers();

    if let Some(parent) = target_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let temp_file = target_path.with_extension(format!("tmp.{}", std::process::id()));

    let mut best_browser = None;
    let mut fallback_success = false;

    for browser in browsers {
        tracing::debug!("Attempting to auto-import YouTube cookies from {browser}...");
        let mut cmd = tokio::process::Command::new(&ytdlp_cmd);
        cmd.kill_on_drop(true);
        cmd.args([
            "--cookies-from-browser",
            browser,
            "--cookies",
            temp_file.to_str().unwrap_or_default(),
            "--playlist-items",
            "0",
            "https://www.youtube.com",
            "--no-warnings",
            "--socket-timeout",
            "5",
        ]);

        if let Ok(output) = cmd.output().await
            && output.status.success()
            && temp_file.exists()
        {
            if has_valid_youtube_session(&temp_file) {
                best_browser = Some(browser);
                break;
            } else if !fallback_success
                && temp_file.metadata().is_ok_and(|m| m.len() > 0)
                && has_any_youtube_cookies(&temp_file)
            {
                fallback_success = true;
            }
        }
    }

    if let Some(browser) = best_browser {
        if tokio::fs::rename(&temp_file, &target_path).await.is_err() {
            tokio::fs::copy(&temp_file, &target_path).await?;
            let _ = tokio::fs::remove_file(&temp_file).await;
        }
        reload_youtube_cookie_header();
        tracing::info!(
            "Automatically imported YouTube cookies from {browser} to {}",
            target_path.display()
        );
        return Ok(target_path);
    }

    if fallback_success {
        if tokio::fs::rename(&temp_file, &target_path).await.is_err() {
            tokio::fs::copy(&temp_file, &target_path).await?;
            let _ = tokio::fs::remove_file(&temp_file).await;
        }
        reload_youtube_cookie_header();
        tracing::warn!(
            "Imported guest YouTube cookies into {} (no authenticated browser session found)",
            target_path.display()
        );
        return Ok(target_path);
    }

    let _ = tokio::fs::remove_file(&temp_file).await;

    if target_path.exists() {
        return Ok(target_path);
    }

    Err(YoutubeAudioError::CookieExtractionFailed)
}

/// Ensure YouTube cookies are available, auto-importing from browsers if missing or unauthenticated.
pub async fn ensure_youtube_cookies_available() -> Option<PathBuf> {
    if Path::new("./cookies.txt").exists() && has_valid_youtube_session(Path::new("./cookies.txt"))
    {
        return Some(PathBuf::from("./cookies.txt"));
    }

    let target_path = resolve_cookie_path();
    if target_path.exists() && has_valid_youtube_session(&target_path) {
        return Some(target_path);
    }

    match auto_import_cookies(false).await {
        Ok(path) => Some(path),
        Err(e) => {
            tracing::debug!("Automatic cookie discovery finished: {e}");
            if target_path.exists() {
                Some(target_path)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_candidate_browsers_not_empty() {
        let browsers = candidate_browsers();
        assert!(
            !browsers.is_empty(),
            "Candidate browsers should not be empty"
        );
    }

    #[test]
    fn test_has_valid_youtube_session_detection() {
        let temp_dir = std::env::temp_dir().join(format!("vortex_test_{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let guest_file = temp_dir.join("guest.txt");
        fs::write(
            &guest_file,
            "# Netscape\n.youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tf1=1\n.youtube.com\tTRUE\t/\tTRUE\t0\tVISITOR_INFO1_LIVE\tabc\n",
        )
        .unwrap();

        assert!(!has_valid_youtube_session(&guest_file));
        assert!(has_any_youtube_cookies(&guest_file));

        let auth_file = temp_dir.join("auth.txt");
        fs::write(
            &auth_file,
            "# Netscape\n.youtube.com\tTRUE\t/\tTRUE\t0\tPREF\tf1=1\n.youtube.com\tTRUE\t/\tTRUE\t0\tLOGIN_INFO\tauth_token\n",
        )
        .unwrap();

        assert!(has_valid_youtube_session(&auth_file));
        assert!(has_any_youtube_cookies(&auth_file));

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
