//! Byte/age formatting (KiB base). Spec: plan §2.14 / CLI-UX §2.2.

use std::time::Duration;

/// KiB base: `"942 B"` (<1024, integer), `"1.2 GiB"`, `"612 MiB"`,
/// `"148.2 GiB"`.
/// 1 decimal when fraction; trailing `.0` trimmed (`"612.0 MiB"` →
/// `"612 MiB"`); 1024 → `"1 KiB"`.
/// Promote when rounding reaches 1024.0: 1023.94 KiB → `"1 MiB"`.
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
        // Promote when rounding would hit the next unit's threshold:
        // 1023.94 KiB rounds to 1024.0 KiB → show 1 MiB instead.
        if unit > 0 && (value * 10.0).round() / 10.0 >= 1024.0 && unit + 1 < UNITS.len() {
            value /= 1024.0;
            unit += 1;
        }
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        let rounded = (value * 10.0).round() / 10.0;
        if (rounded - rounded.trunc()).abs() < f64::EPSILON {
            format!("{} {}", rounded.trunc() as u64, UNITS[unit])
        } else {
            format!("{rounded} {}", UNITS[unit])
        }
    }
}

/// `<60 s` → `"{s}s"`; `<48 h` → `"{h}h"` (at least 1h — anything past a
/// minute reads as hours); else `"{d}d"`; `None` → `"never"`.
pub fn format_age(age: Option<Duration>) -> String {
    let Some(d) = age else {
        return "never".to_string();
    };
    let secs = d.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 48 * 3600 {
        format!("{}h", (secs / 3600).max(1))
    } else {
        format!("{}d", secs / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_boundaries() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(942), "942 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1024), "1 KiB");
        assert_eq!(format_bytes(1_048_576), "1 MiB");
        assert_eq!(format_bytes(612 * 1024 * 1024), "612 MiB");
        assert_eq!(format_bytes(1_288_490_188), "1.2 GiB");
        // Promote on rounding: 1023.94 KiB → 1 MiB.
        assert_eq!(format_bytes(1_048_574), "1 MiB");
        assert_eq!(
            format_bytes(148 * 1024 * 1024 * 1024 + 214_748_365),
            "148.2 GiB"
        );
        assert_eq!(
            format_bytes(40 * 1024 * 1024 * 1024 + 214_748_365),
            "40.2 GiB"
        );
    }

    #[test]
    fn ages() {
        assert_eq!(format_age(Some(Duration::from_secs(0))), "0s");
        assert_eq!(format_age(Some(Duration::from_secs(59))), "59s");
        assert_eq!(format_age(Some(Duration::from_secs(60))), "1h");
        assert_eq!(format_age(Some(Duration::from_secs(3600))), "1h");
        assert_eq!(format_age(Some(Duration::from_secs(47 * 3600))), "47h");
        assert_eq!(format_age(Some(Duration::from_secs(48 * 3600))), "2d");
        assert_eq!(format_age(Some(Duration::from_secs(3 * 86_400))), "3d");
        assert_eq!(format_age(None), "never");
    }
}
