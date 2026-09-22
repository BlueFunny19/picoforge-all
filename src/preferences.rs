//! Persistent software preferences, separate from device configuration.
use serde::{Deserialize, Serialize};
use std::sync::{LazyLock, RwLock};
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Preferences {
    pub language: String,
    pub time_zone: String,
    #[serde(rename = "utc_time", skip_serializing)]
    legacy_utc_time: bool,
    pub sync_clock: bool,
    pub log_level: String,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            language: "system".into(),
            time_zone: "system".into(),
            legacy_utc_time: false,
            sync_clock: true,
            log_level: "INFO".into(),
        }
    }
}
static CURRENT: LazyLock<RwLock<Preferences>> =
    LazyLock::new(|| RwLock::new(Preferences::default()));
pub fn get() -> Preferences {
    CURRENT.read().unwrap().clone()
}
fn path() -> Result<std::path::PathBuf, String> {
    directories::ProjectDirs::from("org", "PicoForge", "PicoForge All")
        .map(|d| d.config_dir().join("settings.json"))
        .ok_or_else(|| "Software settings directory is unavailable".into())
}
fn system_chinese() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(buffer: *mut u16, length: i32) -> i32;
        }
        let mut buffer = [0u16; 85];
        let n = unsafe { GetUserDefaultLocaleName(buffer.as_mut_ptr(), 85) };
        return n > 1
            && String::from_utf16_lossy(&buffer[..n as usize - 1])
                .to_ascii_lowercase()
                .starts_with("zh");
    }
    #[cfg(not(windows))]
    {
        std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .starts_with("zh")
    }
}
fn apply(p: Preferences) {
    crate::i18n::set_chinese(p.language == "zh_CN" || (p.language == "system" && system_chinese()));
    *CURRENT.write().unwrap() = p;
}
pub fn load() {
    let p = path()
        .ok()
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice::<Preferences>(&b).ok())
        .unwrap_or_default();
    apply(p.normalized());
}
impl Preferences {
    fn normalized(mut self) -> Self {
        if self.legacy_utc_time && self.time_zone == "system" {
            self.time_zone = "UTC".into();
        }
        self.legacy_utc_time = false;
        if self.time_zone != "system" && self.time_zone.parse::<chrono_tz::Tz>().is_err() {
            self.time_zone = "system".into();
        }
        if !["system", "en_US", "zh_CN"].contains(&self.language.as_str()) {
            self.language = "system".into();
        }
        if !["ERROR", "WARN", "INFO", "DEBUG", "TRACE"].contains(&self.log_level.as_str()) {
            self.log_level = "INFO".into();
        }
        self
    }
}
pub fn save(p: Preferences) -> Result<(), String> {
    let p = p.normalized();
    let path = path()?;
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(
        path,
        serde_json::to_vec_pretty(&p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    apply(p);
    Ok(())
}
pub fn list_height(count: usize, row: f32) -> f32 {
    (count as f32 * row).min((3. * row).min(600.))
}
pub fn format_time(time: chrono::DateTime<chrono::Utc>, pattern: &str) -> String {
    format_in_zone(time, &get().time_zone, pattern)
}
fn format_in_zone(time: chrono::DateTime<chrono::Utc>, zone: &str, pattern: &str) -> String {
    if let Ok(zone) = zone.parse::<chrono_tz::Tz>() {
        time.with_timezone(&zone).format(pattern).to_string()
    } else {
        time.with_timezone(&chrono::Local)
            .format(pattern)
            .to_string()
    }
}
pub fn timestamp(seconds: u32) -> Option<String> {
    let time = chrono::DateTime::from_timestamp(seconds as i64, 0)?;
    Some(format_time(time, "%Y-%m-%d %H:%M:%S %:z"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn named_zones_use_date_specific_offsets() {
        let winter = "2026-01-15T12:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        let summer = "2026-07-15T12:00:00Z"
            .parse::<chrono::DateTime<chrono::Utc>>()
            .unwrap();
        let pattern = "%Y-%m-%d %H:%M:%S %:z";
        assert_eq!(
            format_in_zone(winter, "Asia/Shanghai", pattern),
            "2026-01-15 20:00:00 +08:00"
        );
        assert_eq!(
            format_in_zone(winter, "Asia/Kathmandu", pattern),
            "2026-01-15 17:45:00 +05:45"
        );
        assert_eq!(
            format_in_zone(winter, "America/New_York", pattern),
            "2026-01-15 07:00:00 -05:00"
        );
        assert_eq!(
            format_in_zone(summer, "America/New_York", pattern),
            "2026-07-15 08:00:00 -04:00"
        );
        assert_eq!(
            format_in_zone(winter, "UTC", pattern),
            "2026-01-15 12:00:00 +00:00"
        );
    }
    #[test]
    fn timezone_settings_migrate_and_round_trip() {
        for (json, expected) in [
            (r#"{"utc_time":true}"#, "UTC"),
            (r#"{"utc_time":false}"#, "system"),
            (
                r#"{"time_zone":"Asia/Shanghai","utc_time":true}"#,
                "Asia/Shanghai",
            ),
            (r#"{"time_zone":"invalid"}"#, "system"),
        ] {
            let p = serde_json::from_str::<Preferences>(json)
                .unwrap()
                .normalized();
            assert_eq!(p.time_zone, expected);
            let json = serde_json::to_value(&p).unwrap();
            assert!(json.get("utc_time").is_none());
            assert_eq!(
                serde_json::from_value::<Preferences>(json)
                    .unwrap()
                    .normalized(),
                p
            );
        }
    }
    #[test]
    fn partial_settings_preserve_defaults_and_validate_values() {
        let p: Preferences =
            serde_json::from_str(
                r#"{"language":"zh_CN","log_level":"INVALID","list_rows":8,"raw_logs":true,"log_follow":false}"#,
            ).unwrap();
        let p = p.normalized();
        assert_eq!(p.log_level, "INFO");
        let saved = serde_json::to_value(&p).unwrap();
        for removed in ["list_rows", "raw_logs", "log_follow"] {
            assert!(saved.get(removed).is_none());
        }
        assert!(p.sync_clock);
        assert_eq!(p.language, "zh_CN");
        assert_eq!(
            serde_json::from_slice::<Preferences>(&serde_json::to_vec(&p).unwrap()).unwrap(),
            p
        );
    }
}
