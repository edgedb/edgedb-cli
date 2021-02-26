use std::borrow::Cow;
use std::time::{SystemTime, Duration};
use std::fmt;


pub fn done_before(timestamp: SystemTime) -> impl fmt::Display {
    timestamp.elapsed()
        .map(|duration| {
            let min = Duration::new(duration.as_secs() / 60 * 60, 0);
            Cow::Owned(format!("done {} ago", humantime::format_duration(min)))
        })
        .unwrap_or_else(|_| Cow::Borrowed("done just now"))
}
