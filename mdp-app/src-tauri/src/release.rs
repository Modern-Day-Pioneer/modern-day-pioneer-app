//! Public test (sprint 12): "Send feedback" and the "a newer version is
//! available" note. Nothing is installed automatically; the note links to
//! the public releases page, where the person downloads the new installer.

use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::{err, load_device, save_device, Res};

/// The public code mirror; testers download from its Releases.
pub const RELEASES_REPO: &str = "Modern-Day-Pioneer/modern-day-pioneer-app";
/// Where "Send feedback" writes to. Empty hides the button.
pub const FEEDBACK_EMAIL: &str = "mbwamkali@hotmail.com";

pub fn releases_page() -> String {
    format!("https://github.com/{RELEASES_REPO}/releases")
}

#[derive(Serialize)]
pub struct Update {
    pub version: String,
    pub page: String,
}

#[derive(Deserialize)]
struct Latest {
    tag_name: String,
    html_url: Option<String>,
}

/// "0.12.72" or "v0.12.72" as numbers, to compare.
fn parts(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches(['v', 'V']).split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect()
}

pub fn newer(latest: &str, current: &str) -> bool {
    let (a, b) = (parts(latest), parts(current));
    !a.is_empty() && a > b
}

/// The web client for the update check and the model download: the
/// computer's own trusted certificates on the desktop, built-in ones on the phone.
pub fn web() -> ureq::Agent {
    use ureq::tls::{RootCerts, TlsConfig};
    let roots = if cfg!(target_os = "android") { RootCerts::WebPki } else { RootCerts::PlatformVerifier };
    ureq::Agent::config_builder().tls_config(TlsConfig::builder().root_certs(roots).build()).build().new_agent()
}

/// The newest public release, or None while there is none (a 404).
fn latest_release() -> Result<Option<Latest>, String> {
    let resp = web().get(&format!("https://api.github.com/repos/{RELEASES_REPO}/releases/latest"))
        .header("User-Agent", "ModernDayPioneer")
        .header("Accept", "application/vnd.github+json")
        .config()
        .timeout_global(Some(std::time::Duration::from_secs(10)))
        .build()
        .call();
    match resp {
        Ok(r) => serde_json::from_reader(r.into_body().into_reader()).map(Some).map_err(err),
        Err(ureq::Error::StatusCode(404)) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Ask the releases page for the newest version. `daily`: at most once a
/// day (the check on opening); the button in Settings always asks.
#[tauri::command]
pub async fn check_for_update(app: AppHandle, daily: bool) -> Res<Option<Update>> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    if daily && load_device(&app).update_checked.as_deref() == Some(today.as_str()) {
        return Ok(None);
    }
    // The day counts as checked only once the page answered (an answer of
    // "no release yet" included). Offline, the next launch tries again; the
    // screens ask once per launch, so this never repeats in a loop.
    let mark_checked = || -> Res<()> {
        if daily {
            let mut d = load_device(&app);
            d.update_checked = Some(today.clone());
            save_device(&app, &d)?;
        }
        Ok(())
    };
    let current = app.package_info().version.to_string();
    let latest = match latest_release() {
        Ok(Some(latest)) => {
            mark_checked()?;
            latest
        }
        // No release published yet.
        Ok(None) => {
            mark_checked()?;
            return Ok(None);
        }
        Err(e) => return Err(format!("The releases page could not be reached ({e}).")),
    };
    Ok(newer(&latest.tag_name, &current).then(|| Update {
        version: latest.tag_name.trim_start_matches(['v', 'V']).to_string(),
        page: latest.html_url.unwrap_or_else(releases_page),
    }))
}

/// Google's page for asking for a Takeout with only Google Photos selected (the "Bring in from Google Photos" steps).
pub const TAKEOUT_PAGE: &str = "https://takeout.google.com/settings/takeout/custom/photos";

/// The project's website; Help pages live under /help/ (sprint 14).
pub const SITE: &str = "https://moderndaypioneer.org/";

/// Open the releases page, the project's website, Google Takeout or a
/// feedback email; nothing else.
#[tauri::command]
pub fn open_link(app: AppHandle, url: String) -> Res<()> {
    let ok = url.starts_with(&releases_page()) || url.starts_with(SITE) || url == TAKEOUT_PAGE || (!FEEDBACK_EMAIL.is_empty() && url.starts_with(&format!("mailto:{FEEDBACK_EMAIL}")));
    if !ok {
        return Err("That link can't be opened from here.".into());
    }
    app.opener().open_url(url, None::<&str>).map_err(err)
}

#[cfg(test)]
mod tests {
    /// Needs the internet: asks the real public releases page.
    #[test]
    #[ignore]
    fn reads_the_latest_release() {
        let latest = super::latest_release().unwrap().expect("a public release");
        assert!(super::newer(&latest.tag_name, "0.0.1"), "{}", latest.tag_name);
        assert!(latest.html_url.unwrap().contains("/releases/"));
    }

    #[test]
    fn versions() {
        assert!(super::newer("v0.12.80", "0.12.72"));
        assert!(super::newer("0.13.1", "0.12.99"));
        assert!(!super::newer("v0.12.72", "0.12.72"));
        assert!(!super::newer("0.12.9", "0.12.72"));
        assert!(!super::newer("nonsense", "0.12.72"));
    }
}
