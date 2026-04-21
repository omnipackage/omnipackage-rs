use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_help() {
    Command::cargo_bin("omnipackage").unwrap().arg("--help").assert().success().stdout(predicate::str::contains("Usage"));
}

#[test]
fn test_version() {
    Command::cargo_bin("omnipackage")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn test_build_help() {
    Command::cargo_bin("omnipackage")
        .unwrap()
        .arg("build")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--distros"));
}

#[test]
fn test_build_dir_default() {
    Command::cargo_bin("omnipackage")
        .unwrap()
        .args(["build", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(std::env::temp_dir().to_string_lossy().as_ref()));
}

#[test]
fn test_gpg_generate() {
    Command::cargo_bin("omnipackage")
        .unwrap()
        .args(["gpg", "generate", "--name", "John Doe", "--email", "john@example.com"])
        .assert()
        .success()
        .stdout(predicate::str::contains("BEGIN PGP PRIVATE KEY BLOCK"))
        .stdout(predicate::str::contains("BEGIN PGP PUBLIC KEY BLOCK").not());
}
/* TODO integration test with smaple_project in fixtures
#[test]
fn test_build_dir_custom() {
    Command::cargo_bin("omnipackage")
        .unwrap()
        .args(["build", ".", "--build-dir", "/tmp/custom"])
        .assert()
        .success();
}*/
