//! Capture timestamps for entries created by annotation.
//!
//! Pixel filenames (`PXL_YYYYMMDD_HHMMSSmmm`) carry the capture time in UTC,
//! while EXIF `DateTimeOriginal` — used for every photo that had GPS — is local
//! time. Annotated entries must use the same clock or time-ordered navigation
//! interleaves wrongly, so convert the filename's UTC to trip-local time.
//! (photo-extract's `utc_to_trip_local` is the twin of this for ffprobe.)

/// Offset of local time from UTC where the photos were taken (Greece in
/// summer, EEST). Every EXIF timestamp in the set is exactly this far ahead of
/// its filename.
pub const TRIP_UTC_OFFSET_HOURS: i64 = 3;

/// `PXL_20260619_052531747.MP.jpg` -> `2026:06:19 08:25:31`.
pub fn local_timestamp_from_filename(filename: &str) -> Option<String> {
    let rest = filename.strip_prefix("PXL_")?;
    let digits = |s: &str| -> Option<i64> {
        if s.bytes().all(|b| b.is_ascii_digit()) {
            s.parse().ok()
        } else {
            None
        }
    };
    let (y, mo, d) = (
        digits(rest.get(0..4)?)?,
        digits(rest.get(4..6)?)?,
        digits(rest.get(6..8)?)?,
    );
    if rest.get(8..9)? != "_" {
        return None;
    }
    let (h, mi, s) = (
        digits(rest.get(9..11)?)?,
        digits(rest.get(11..13)?)?,
        digits(rest.get(13..15)?)?,
    );
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 60 {
        return None;
    }

    let secs = days_from_civil(y, mo, d) * 86_400
        + h * 3_600
        + mi * 60
        + s
        + TRIP_UTC_OFFSET_HOURS * 3_600;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (y, mo, d) = civil_from_days(days);
    Some(format!(
        "{y:04}:{mo:02}:{d:02} {:02}:{:02}:{:02}",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    ))
}

// Howard Hinnant's days <-> civil date algorithms (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_utc_filename_to_local() {
        // Matches the EXIF DateTimeOriginal of this photo in photo_data.json.
        assert_eq!(
            local_timestamp_from_filename("PXL_20260619_114518677.jpg").as_deref(),
            Some("2026:06:19 14:45:18")
        );
        assert_eq!(
            local_timestamp_from_filename("PXL_20260626_152447344.LS.mp4").as_deref(),
            Some("2026:06:26 18:24:47")
        );
    }

    #[test]
    fn rolls_over_month_end() {
        assert_eq!(
            local_timestamp_from_filename("PXL_20260630_223000000.jpg").as_deref(),
            Some("2026:07:01 01:30:00")
        );
    }

    #[test]
    fn rejects_other_names() {
        assert_eq!(local_timestamp_from_filename("IMG_1234.jpg"), None);
        assert_eq!(local_timestamp_from_filename("PXL_2026.jpg"), None);
        assert_eq!(
            local_timestamp_from_filename("PXL_20261340_000000.jpg"),
            None
        );
    }
}
