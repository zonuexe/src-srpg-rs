//! SRC 時間関数 (`Now()` / `Year()` / `Month()` / `Day()` / `Hour()` /
//! `Minute()` / `Second()` / `Weekday()` / `DiffTime()`) 向け、Unix epoch
//! ミリ秒と Gregorian カレンダの相互変換。
//!
//! プラットフォーム独立に書くため `chrono` / `time` 等を使わず、Howard
//! Hinnant の `civil_from_days` アルゴリズム (パブリックドメイン) を
//! 直接実装している (`https://howardhinnant.github.io/date_algorithms.html`)。
//! 1970-01-01 を 0 とした「days since unix epoch」を年・月・日に分解する。

/// 日数 (Unix epoch からの) → (year, month, day) の Gregorian 分解。
/// 月は 1..=12、日は 1..=31。
/// Howard Hinnant `civil_from_days` 移植。
pub fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719468;
    let era = if z >= 0 {
        z / 146097
    } else {
        (z - 146096) / 146097
    };
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Unix epoch ms → (Y, M, D, h, m, s, weekday)。weekday は 0=日 .. 6=土。
pub fn breakdown(epoch_ms: f64) -> Breakdown {
    let secs = (epoch_ms / 1000.0).floor() as i64;
    let days = secs.div_euclid(86400);
    let time_of_day = secs.rem_euclid(86400) as u32;
    let (y, m, d) = civil_from_days(days);
    // 1970-01-01 は木曜 = weekday 4。
    let wd = ((days + 4).rem_euclid(7)) as u32;
    Breakdown {
        year: y,
        month: m,
        day: d,
        hour: time_of_day / 3600,
        minute: (time_of_day / 60) % 60,
        second: time_of_day % 60,
        weekday: wd,
    }
}

/// `Now()` が返す文字列フォーマット。SRC.NET の `DateAndTime.Now.ToString()`
/// に近い `YYYY/MM/DD HH:MM:SS` 形式 (日本ロケール風)。
pub fn format_now(epoch_ms: f64) -> String {
    let b = breakdown(epoch_ms);
    format!(
        "{:04}/{:02}/{:02} {:02}:{:02}:{:02}",
        b.year, b.month, b.day, b.hour, b.minute, b.second
    )
}

/// `Weekday()` の日本語綴: 日曜 / 月曜 / ... / 土曜。
pub fn weekday_name(weekday: u32) -> &'static str {
    match weekday {
        0 => "日曜",
        1 => "月曜",
        2 => "火曜",
        3 => "水曜",
        4 => "木曜",
        5 => "金曜",
        6 => "土曜",
        _ => "",
    }
}

/// `format_now` 形式 + ISO 8601 形式 (`YYYY-MM-DDTHH:MM:SS`) の両対応パーサ。
/// 不正な文字列は `None` を返す。返値は Unix epoch ミリ秒。
pub fn parse_datetime(s: &str) -> Option<f64> {
    let s = s.trim();
    // 区切り文字を `/` / `-` / `T` / 空白 にほぼ寛容に。
    let mut digits: Vec<i64> = Vec::with_capacity(6);
    let mut cur = String::new();
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            digits.push(cur.parse().ok()?);
            cur.clear();
        }
    }
    if !cur.is_empty() {
        digits.push(cur.parse().ok()?);
    }
    if digits.len() < 3 {
        return None;
    }
    let y = digits[0] as i32;
    let m = digits[1] as u32;
    let d = digits[2] as u32;
    let h = digits.get(3).copied().unwrap_or(0) as u32;
    let mi = digits.get(4).copied().unwrap_or(0) as u32;
    let se = digits.get(5).copied().unwrap_or(0) as u32;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some(epoch_ms_from(y, m, d, h, mi, se))
}

/// (Y, M, D, h, m, s) → Unix epoch ミリ秒。Hinnant `days_from_civil` 移植。
pub fn epoch_ms_from(year: i32, month: u32, day: u32, hour: u32, minute: u32, second: u32) -> f64 {
    let y = if month <= 2 { year - 1 } else { year } as i64;
    let m = if month <= 2 { month + 9 } else { month - 3 } as i64;
    let era = if y >= 0 { y / 400 } else { (y - 399) / 400 };
    let yoe = (y - era * 400) as u64;
    let doy = (153 * m as u64 + 2) / 5 + day as u64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe as i64 - 719468;
    let secs = days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    secs as f64 * 1000.0
}

pub struct Breakdown {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub weekday: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_is_1970_01_01_thursday() {
        let b = breakdown(0.0);
        assert_eq!((b.year, b.month, b.day), (1970, 1, 1));
        assert_eq!((b.hour, b.minute, b.second), (0, 0, 0));
        assert_eq!(b.weekday, 4, "1970-01-01 は木曜");
    }

    #[test]
    fn format_now_zero_is_iso_like() {
        assert_eq!(format_now(0.0), "1970/01/01 00:00:00");
    }

    #[test]
    fn roundtrip_2024_02_29_leap_year() {
        // 2024 はうるう年
        let ms = epoch_ms_from(2024, 2, 29, 12, 34, 56);
        let b = breakdown(ms);
        assert_eq!((b.year, b.month, b.day), (2024, 2, 29));
        assert_eq!((b.hour, b.minute, b.second), (12, 34, 56));
    }

    #[test]
    fn parse_datetime_slash_format() {
        let ms = parse_datetime("2024/02/29 12:34:56").unwrap();
        let b = breakdown(ms);
        assert_eq!(
            (b.year, b.month, b.day, b.hour, b.minute, b.second),
            (2024, 2, 29, 12, 34, 56)
        );
    }

    #[test]
    fn parse_datetime_iso_format() {
        let ms = parse_datetime("2024-02-29T12:34:56").unwrap();
        let b = breakdown(ms);
        assert_eq!((b.year, b.month, b.day), (2024, 2, 29));
    }

    #[test]
    fn parse_datetime_date_only() {
        let ms = parse_datetime("2024-12-25").unwrap();
        let b = breakdown(ms);
        assert_eq!(
            (b.year, b.month, b.day, b.hour, b.minute, b.second),
            (2024, 12, 25, 0, 0, 0)
        );
    }

    #[test]
    fn parse_datetime_invalid_returns_none() {
        assert_eq!(parse_datetime("not a date"), None);
        assert_eq!(parse_datetime("2024"), None); // 月日が無い
        assert_eq!(parse_datetime("2024/13/01"), None); // 月が範囲外
    }

    #[test]
    fn weekday_names_cycle() {
        assert_eq!(weekday_name(0), "日曜");
        assert_eq!(weekday_name(6), "土曜");
    }
}
