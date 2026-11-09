//! What the agent surface may return (`Settings.mcp`), and the time parsing its tools share.
//! Pure: every rule here is unit-tested without a database.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime, TimeZone};
use tuldmi_engine::types::{McpSettings, Source};

/// The stored MCP settings at the moment of a request, applied to discovery and re-checked on
/// every call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceiling {
    pub enabled: bool,
    pub include_remote: bool,
    pub allow_audio: bool,
    /// Oldest start time (epoch ms) any tool may return; `None` = no limit.
    pub oldest: Option<i64>,
}

impl Ceiling {
    pub fn from_settings(settings: &McpSettings, now_ms: i64) -> Self {
        let oldest = (settings.max_age_days > 0).then(|| now_ms.saturating_sub(i64::from(settings.max_age_days) * 86_400_000));
        Ceiling {
            enabled: settings.enabled,
            include_remote: settings.include_remote,
            allow_audio: settings.allow_audio,
            oldest,
        }
    }

    /// Access off: what an unreadable history or settings mean.
    pub fn off() -> Self {
        Ceiling::from_settings(&McpSettings::default(), 0)
    }

    /// Whether an event from `source` that started at `at` may be returned.
    pub fn admits(&self, source: Source, at: i64) -> bool {
        (source == Source::Mic || self.include_remote) && self.oldest.is_none_or(|oldest| at >= oldest)
    }

    /// The store filter covering exactly the admitted sources (empty = all).
    pub fn sources(&self) -> Vec<Source> {
        if self.include_remote { Vec::new() } else { vec![Source::Mic] }
    }

    /// `from` raised to the window's start.
    pub fn clamp_from(&self, from: Option<i64>) -> Option<i64> {
        match (from, self.oldest) {
            (Some(f), Some(o)) => Some(f.max(o)),
            (f, o) => f.or(o),
        }
    }
}

/// How tools name a source: what the person sees in the app.
pub fn source_name(source: Source) -> &'static str {
    match source {
        Source::Mic => "me",
        Source::System => "remote",
    }
}

pub fn parse_source(text: &str) -> Option<Source> {
    match text.trim() {
        "me" => Some(Source::Mic),
        "remote" => Some(Source::System),
        _ => None,
    }
}

/// A time an agent passes: RFC 3339 (`2026-11-07T14:05:00-05:00`), local `2026-11-07 14:05[:30]`
/// or `2026-11-07`, or a duration ago (`30m`, `2h`, `7d`). Returns epoch ms.
pub fn parse_time(text: &str, now: DateTime<Local>) -> Result<i64, String> {
    let t = text.trim();
    for (suffix, unit_ms) in [("m", 60_000_i64), ("h", 3_600_000), ("d", 86_400_000)] {
        if let Some(n) = t.strip_suffix(suffix).and_then(|n| n.parse::<i64>().ok()) {
            const MAX_AGO_MS: i64 = 100 * 366 * 86_400_000;
            return n
                .checked_mul(unit_ms)
                .filter(|ago| (0..=MAX_AGO_MS).contains(ago))
                .and_then(|ago| now.timestamp_millis().checked_sub(ago))
                .ok_or_else(|| format!("'{}' is not a duration between 0 and 100 years", clip(t, 40)));
        }
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(t) {
        return Ok(dt.timestamp_millis());
    }
    let local = |naive: NaiveDateTime| Local.from_local_datetime(&naive).earliest().map(|dt| dt.timestamp_millis());
    for format in ["%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M", "%Y-%m-%dT%H:%M:%S", "%Y-%m-%dT%H:%M"] {
        if let Ok(naive) = NaiveDateTime::parse_from_str(t, format)
            && let Some(ms) = local(naive)
        {
            return Ok(ms);
        }
    }
    if let Ok(date) = NaiveDate::parse_from_str(t, "%Y-%m-%d")
        && let Some(ms) = date.and_hms_opt(0, 0, 0).and_then(local)
    {
        return Ok(ms);
    }
    Err(format!(
        "could not read the time '{}': use RFC 3339, 'YYYY-MM-DD HH:MM', 'YYYY-MM-DD', or a duration ago like 30m, 2h, 7d",
        clip(t, 40)
    ))
}

/// RFC 3339 in local time, what tools return.
pub fn time_text(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false))
        .unwrap_or_default()
}

pub fn clip(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(enabled: bool, include_remote: bool, days: u32) -> McpSettings {
        McpSettings { enabled, include_remote, allow_audio: false, max_age_days: days }
    }

    #[test]
    fn remote_and_age_are_both_enforced() {
        let now = 100 * 86_400_000;
        let c = Ceiling::from_settings(&settings(true, false, 30), now);
        assert!(c.admits(Source::Mic, now - 86_400_000));
        assert!(!c.admits(Source::System, now - 86_400_000), "Remote is off");
        assert!(!c.admits(Source::Mic, now - 31 * 86_400_000), "older than 30 days");
        assert_eq!(c.sources(), vec![Source::Mic]);
        assert_eq!(c.clamp_from(None), Some(now - 30 * 86_400_000));
        assert_eq!(c.clamp_from(Some(now - 86_400_000)), Some(now - 86_400_000));
        assert_eq!(c.clamp_from(Some(0)), Some(now - 30 * 86_400_000));

        let all = Ceiling::from_settings(&settings(true, true, 0), now);
        assert!(all.admits(Source::System, 0));
        assert!(all.sources().is_empty());
        assert_eq!(all.clamp_from(None), None);
    }

    #[test]
    fn defaults_are_off_and_text_only() {
        let c = Ceiling::off();
        assert!(!c.enabled && !c.include_remote && !c.allow_audio);
    }

    #[test]
    fn times_parse_in_every_documented_form() {
        let now = Local.with_ymd_and_hms(2026, 11, 7, 12, 0, 0).unwrap();
        let ms = now.timestamp_millis();
        assert_eq!(parse_time("30m", now), Ok(ms - 30 * 60_000));
        assert_eq!(parse_time("2h", now), Ok(ms - 2 * 3_600_000));
        assert_eq!(parse_time("7d", now), Ok(ms - 7 * 86_400_000));
        assert_eq!(parse_time("2026-11-07 12:00", now), Ok(ms));
        assert_eq!(parse_time("2026-11-07T12:00:00", now), Ok(ms));
        assert_eq!(parse_time("2026-11-07", now), Ok(ms - 12 * 3_600_000));
        assert_eq!(parse_time("2026-11-07T17:00:00Z", now), Ok(DateTime::parse_from_rfc3339("2026-11-07T17:00:00Z").unwrap().timestamp_millis()));
        assert!(parse_time("-5m", now).is_err());
        assert!(parse_time("yesterday", now).is_err());
        assert_eq!(parse_source("me"), Some(Source::Mic));
        assert_eq!(parse_source("remote"), Some(Source::System));
        assert_eq!(parse_source("system"), None);
    }
}
