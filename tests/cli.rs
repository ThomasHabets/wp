use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

fn wp() -> &'static str {
    env!("CARGO_BIN_EXE_wp")
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn shell_quote_path(path: &Path) -> String {
    shell_quote(&path.to_string_lossy())
}

fn test_path(name: &str) -> PathBuf {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let mut path = std::env::current_dir().unwrap();
    path.push("target");
    fs::create_dir_all(&path).unwrap();
    path.push(format!(
        "wp-test-{}-{}-{}",
        name,
        std::process::id(),
        suffix
    ));
    path
}

#[test]
fn failed_child_drains_stdout_before_exit() {
    let wp = shell_quote(wp());
    let output = test_path("drain");
    let producer = shell_quote("dd if=/dev/zero bs=1048576 count=8 2>/dev/null; exit 7");
    let consumer = shell_quote(&format!("cat >{}", shell_quote_path(&output)));
    let command = format!("{wp} -o sh -c {producer} | {wp} -i sh -c {consumer}");

    let result = Command::new("sh").arg("-c").arg(command).output().unwrap();

    assert!(!result.status.success());
    assert_eq!(fs::metadata(&output).unwrap().len(), 8 * 1024 * 1024);
    fs::remove_file(output).unwrap();
}

#[test]
fn trailing_input_after_protocol_eof_is_rejected() {
    let data = test_path("trailing-data");
    let commit = test_path("trailing-commit");
    let script = format!(
        "cat >{}; touch {}",
        shell_quote_path(&data),
        shell_quote_path(&commit)
    );
    let mut child = Command::new(wp())
        .args(["-i", "sh", "-c", &script])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"<payload>Zgarbage")
        .unwrap();

    let status = child.wait().unwrap();
    assert!(!status.success());
    assert!(!commit.exists());

    let _ = fs::remove_file(data);
    let _ = fs::remove_file(commit);
}

#[test]
fn closed_stdout_pipe_does_not_panic() {
    let mut child = Command::new(wp())
        .args(["-o", "sh", "-c", "printf %010000d 0"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let mut byte = [0];
    stdout.read_exact(&mut byte).unwrap();
    drop(stdout);

    let output = child.wait_with_output().unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(!stderr.contains("panicked"));
    assert_ne!(output.status.code(), Some(101));
}
