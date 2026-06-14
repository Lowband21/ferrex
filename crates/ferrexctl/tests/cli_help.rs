use assert_cmd::cargo::cargo_bin_cmd;

#[test]
fn stack_help_mentions_options() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    let output = cmd
        .arg("stack")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&output);
    assert!(text.contains("stack up"), "stack help missing 'stack up'");
    // Detailed flags live on subcommand help; check stack up.
    let mut cmd_up = cargo_bin_cmd!("ferrexctl");
    let up_output = cmd_up
        .arg("stack")
        .arg("up")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let up_text = String::from_utf8_lossy(&up_output);
    assert!(
        up_text.contains("--server"),
        "stack up help missing --server arg"
    );
    assert!(
        up_text.contains("--profile"),
        "stack up help missing --profile"
    );
}

#[test]
fn logs_command_is_documented() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    let out = cmd
        .arg("logs")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("--service"), "logs help missing service flag");
}

#[test]
fn db_subcommands_present() {
    let mut cmd = cargo_bin_cmd!("ferrexctl");
    let out = cmd
        .arg("db")
        .arg("--help")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8_lossy(&out);
    assert!(text.contains("preflight"), "db help missing preflight");
    assert!(text.contains("migrate"), "db help missing migrate");
}
