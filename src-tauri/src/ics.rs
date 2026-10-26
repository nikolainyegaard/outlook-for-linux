//! Opening an .ics file: the first VEVENT is read and Outlook's calendar
//! compose deep link is opened with its fields, the way a mailto: link
//! opens a prefilled message. The event is saved by the user. Lost on the
//! way: recurrence rules, and the file's meaning as an invitation (it
//! becomes a new event, not an RSVP). The deep link is the one "Add to
//! calendar" buttons use.

use std::path::{Path, PathBuf};

use tauri::Url;

const CALENDAR_COMPOSE_URL: &str = "https://outlook.office.com/calendar/0/deeplink/compose";

/// Lower-cased argv entry pointing at an .ics: a path or a file:// URL,
/// which is how a desktop launches a MimeType handler with `%u`.
pub fn path_from_arg(arg: &str) -> Option<PathBuf> {
    if !arg.to_ascii_lowercase().ends_with(".ics") {
        return None;
    }
    if let Some(rest) = arg.strip_prefix("file://") {
        let path = rest.split_once('/').map_or(rest, |(_, p)| p);
        return Some(PathBuf::from(format!("/{}", percent_encoding::percent_decode_str(path).decode_utf8_lossy())));
    }
    Some(PathBuf::from(arg))
}

/// The compose URL for the file's first event, or why there is none.
pub fn compose_url_for(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("Cannot read {}: {e}", path.display()))?;
    compose_url(&text).ok_or_else(|| format!("No event found in {}", path.display()))
}

fn compose_url(ics: &str) -> Option<String> {
    let event = first_event(ics)?;
    let mut url = Url::parse(CALENDAR_COMPOSE_URL).expect("compose url parses");
    {
        let mut q = url.query_pairs_mut();
        q.append_pair("path", "/calendar/action/compose");
        q.append_pair("rru", "addevent");
        let start = event.iter().find(|(k, _, _)| k == "DTSTART");
        let all_day = start.is_some_and(|(_, params, value)| params.contains("VALUE=DATE") || value.len() == 8);
        q.append_pair("allday", if all_day { "true" } else { "false" });
        for (key, _, value) in &event {
            match key.as_str() {
                "SUMMARY" => {
                    q.append_pair("subject", value);
                }
                "LOCATION" => {
                    q.append_pair("location", value);
                }
                "DESCRIPTION" => {
                    q.append_pair("body", value);
                }
                "DTSTART" => {
                    q.append_pair("startdt", &iso_datetime(value));
                }
                "DTEND" => {
                    // An all-day DTEND is exclusive; Outlook's is inclusive.
                    let end = if all_day { previous_day(value) } else { iso_datetime(value) };
                    q.append_pair("enddt", &end);
                }
                "ATTENDEE" => {
                    if let Some(mail) = value.strip_prefix("mailto:").or_else(|| value.strip_prefix("MAILTO:")) {
                        q.append_pair("to", mail);
                    }
                }
                _ => {}
            }
        }
    }
    Some(url.into())
}

/// The first VEVENT as (NAME, PARAMS, unescaped value) lines, folded
/// continuation lines joined.
fn first_event(ics: &str) -> Option<Vec<(String, String, String)>> {
    let mut lines: Vec<String> = Vec::new();
    for raw in ics.lines() {
        let raw = raw.trim_end_matches('\r');
        if let Some(rest) = raw.strip_prefix(' ').or_else(|| raw.strip_prefix('\t')) {
            if let Some(last) = lines.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        lines.push(raw.to_string());
    }
    let mut event = Vec::new();
    let mut inside = false;
    for line in lines {
        if line == "BEGIN:VEVENT" {
            inside = true;
            continue;
        }
        if line == "END:VEVENT" {
            return Some(event);
        }
        if !inside {
            continue;
        }
        let Some((head, value)) = line.split_once(':') else { continue };
        let (name, params) = head.split_once(';').unwrap_or((head, ""));
        event.push((name.to_ascii_uppercase(), params.to_ascii_uppercase(), unescape(value)));
    }
    None
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// `20261026T140000Z` to `2026-10-26T14:00:00Z`, `20261026` to `2026-10-26`.
/// A TZID time stays local without its zone, and Outlook reads it in the
/// user's calendar time zone. ponytail: no VTIMEZONE parsing; an event
/// from another zone lands at the wrong hour.
fn iso_datetime(value: &str) -> String {
    let v = value.trim();
    if v.len() < 8 || !v.is_char_boundary(8) {
        return v.to_string();
    }
    let date = format!("{}-{}-{}", &v[0..4], &v[4..6], &v[6..8]);
    match v.get(9..) {
        Some(time) if v.as_bytes().get(8) == Some(&b'T') && time.len() >= 6 => {
            format!("{date}T{}:{}:{}{}", &time[0..2], &time[2..4], &time[4..6], &time[6..])
        }
        _ => date,
    }
}

/// The day before an all-day DTEND, as a date.
fn previous_day(value: &str) -> String {
    let v = value.trim();
    let parsed = (v.get(0..4).and_then(|s| s.parse::<i32>().ok()), v.get(4..6).and_then(|s| s.parse::<u32>().ok()), v.get(6..8).and_then(|s| s.parse::<u32>().ok()));
    let (Some(mut year), Some(mut month), Some(mut day)) = parsed else { return iso_datetime(v) };
    if day > 1 {
        day -= 1;
    } else {
        if month == 1 {
            year -= 1;
            month = 12;
        } else {
            month -= 1;
        }
        let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        day = match month {
            2 if leap => 29,
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
    }
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:1\r\nDTSTART;TZID=Europe/Oslo:20261217T180000\r\nDTEND;TZID=Europe/Oslo:20261217T220000\r\nSUMMARY:Julesamling på Kleivstua\r\nLOCATION:Kleivstua\\, Hole\r\nDESCRIPTION:Noe dere kunne tenke dere å delta på?\\nJeg tenker å dra.\r\n  Med vennlig hilsen\r\nATTENDEE;CN=Nikolai:mailto:nikolai@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";

    #[test]
    fn event_becomes_a_compose_link() {
        let url = Url::parse(&compose_url(SAMPLE).unwrap()).unwrap();
        let q: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        let get = |k: &str| q.iter().find(|(key, _)| key == k).map(|(_, v)| v.as_str());
        assert_eq!(get("subject"), Some("Julesamling på Kleivstua"));
        assert_eq!(get("location"), Some("Kleivstua, Hole"));
        assert_eq!(get("body"), Some("Noe dere kunne tenke dere å delta på?\nJeg tenker å dra. Med vennlig hilsen"));
        assert_eq!(get("startdt"), Some("2026-12-17T18:00:00"));
        assert_eq!(get("enddt"), Some("2026-12-17T22:00:00"));
        assert_eq!(get("to"), Some("nikolai@example.com"));
        assert_eq!(get("allday"), Some("false"));
    }

    #[test]
    fn all_day_and_utc_dates() {
        let ics = "BEGIN:VEVENT\nDTSTART;VALUE=DATE:20260301\nDTEND;VALUE=DATE:20260302\nSUMMARY:Day\nEND:VEVENT\n";
        let url = compose_url(ics).unwrap();
        assert!(url.contains("allday=true"));
        assert!(url.contains("startdt=2026-03-01&"));
        assert!(url.contains("enddt=2026-03-01"));
        assert_eq!(iso_datetime("20261026T140000Z"), "2026-10-26T14:00:00Z");
        assert_eq!(previous_day("20240301"), "2024-02-29");
        assert_eq!(previous_day("20260101"), "2025-12-31");
        assert!(compose_url("BEGIN:VCALENDAR\nEND:VCALENDAR\n").is_none());
    }

    #[test]
    fn argv_forms() {
        assert_eq!(path_from_arg("file:///home/n/My%20invite.ics"), Some(PathBuf::from("/home/n/My invite.ics")));
        assert_eq!(path_from_arg("/tmp/a.ICS"), Some(PathBuf::from("/tmp/a.ICS")));
        assert_eq!(path_from_arg("mailto:a@b.com"), None);
    }
}
