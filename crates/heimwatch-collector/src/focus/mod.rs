#[cfg(target_os = "linux")]
mod linux;

#[cfg(target_os = "linux")]
pub use linux::FocusCollector;

#[cfg(not(target_os = "linux"))]
pub struct FocusCollector;

#[cfg(not(target_os = "linux"))]
impl FocusCollector {
    pub fn try_new() -> Option<Self> {
        None
    }
}
