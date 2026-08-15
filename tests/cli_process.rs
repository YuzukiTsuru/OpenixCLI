use std::path::PathBuf;
use std::process::Command;

fn missing_firmware_path(command: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("missing-{command}-firmware.fex"))
}

#[test]
fn firmware_commands_dispatch_and_reject_a_missing_input() {
    for command in ["flash", "inspect", "unpack", "convert"] {
        let missing = missing_firmware_path(command);
        assert!(!missing.exists(), "test fixture unexpectedly exists");

        let output = Command::new(env!("CARGO_BIN_EXE_openixcli"))
            .arg(command)
            .arg(&missing)
            .output()
            .expect("openixcli process should start");

        assert!(
            !output.status.success(),
            "{command} unexpectedly accepted a missing firmware file"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("Firmware file not found"),
            "unexpected {command} stderr: {stderr}"
        );
    }

    let missing = missing_firmware_path("raw");
    let output = Command::new(env!("CARGO_BIN_EXE_openixcli"))
        .arg("raw")
        .arg(&missing)
        .arg("raw.img")
        .output()
        .expect("openixcli process should start");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Firmware file not found"));
}
