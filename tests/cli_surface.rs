use std::process::Command;

#[test]
fn removed_window_targeting_setup_command_is_not_exposed() {
    let output = Command::new(env!("CARGO_BIN_EXE_computer-use-linux"))
        .arg("setup-window-targeting")
        .output()
        .expect("run computer-use-linux CLI");

    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("unknown command 'setup-window-targeting'"));
}
