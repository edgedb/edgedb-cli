use once_cell::sync::OnceCell;
use serde::Serialize;

mod linux;
mod windows;
mod macos;

#[derive(Clone, Debug)]
pub(in crate::server::detect) struct Lazy<T>(once_cell::sync::OnceCell<T>);

#[derive(Clone, Debug, Serialize)]
pub struct Detect {
    pub os_info: OsInfo,
}

#[derive(Clone, Debug, Serialize)]
pub enum OsInfo {
    Linux(linux::OsInfo),
    Windows(windows::OsInfo),
    Macos(macos::OsInfo),
    Unknown,
}

impl Detect {
    pub fn current_os() -> Detect {
        use OsInfo::*;

        Detect {
            os_info: if cfg!(windows) {
                Windows(windows::OsInfo::new())
            } else if cfg!(macos) {
                Macos(macos::OsInfo::new())
            } else if cfg!(target_os="linux") {
                Linux(linux::OsInfo::new())
            } else {
                Unknown
            },
        }
    }
    pub fn detect_all(&self) {
        use OsInfo::*;
        match &self.os_info {
            Windows(w) => w.detect_all(),
            Macos(m) => m.detect_all(),
            Linux(l) => l.detect_all(),
            Unknown => {}
        }
    }
}

impl<T: Serialize> Serialize for Lazy<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error> where
        S: serde::Serializer
    {
        self.0.get().serialize(serializer)
    }
}

impl<T> Lazy<T> {
    fn lazy() -> Lazy<T> {
        Lazy(OnceCell::new())
    }
    fn get_or_init<F>(&self, f: F) -> &T
        where F: FnOnce() -> T
    {
        self.0.get_or_init(f)
    }
}

pub fn main(_arg: &crate::server::options::Detect)
    -> Result<(), anyhow::Error>
{
    let det = Detect::current_os();
    det.detect_all();
    serde_json::to_writer_pretty(std::io::stdout(), &det)?;
    Ok(())
}
