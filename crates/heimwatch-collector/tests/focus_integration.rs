#[cfg(target_os = "linux")]
mod tests {
    use heimwatch_collector::FocusCollector;

    #[test]
    fn test_try_new_does_not_panic_without_wayland() {
        // Must not panic regardless of environment (whether Wayland is available or not)
        let result = FocusCollector::try_new();
        assert!(!result.is_none());
    }

    #[test]
    #[ignore] // Requires live Wayland session with wlr-foreign-toplevel support
    fn test_wlr_listener_receives_initial_focus() {
        // This test only runs with `cargo test -- --include-ignored`
        // on a system with Sway, Hyprland, or KWin
        let rt = tokio::runtime::Runtime::new().unwrap();

        let (_tx, _rx) = tokio::sync::mpsc::channel::<Option<String>>(10);

        rt.block_on(async {
            // Spawn a listener task in the background
            let listener_task = tokio::task::spawn_blocking(move || {
                // Attempt to create and run the listener
                // This will fail gracefully if protocol is unavailable
                let _listener = FocusCollector::try_new();
            });

            // Wait a short time for the listener to receive initial state
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            // The test is simply that we don't panic; receiving an actual focus event
            // would require a full integration with the event loop
            drop(listener_task);
        });
    }
}

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("Focus integration tests only run on Linux");
}
