use std::process::Command;

fn rustjvm() -> Command {
    Command::new(env!("CARGO_BIN_EXE_rustjvm"))
}

#[test]
fn new_scaffolds_runnable_project() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("hello-world");

    let status = rustjvm()
        .arg("new")
        .arg(&project)
        .status()
        .expect("rustjvm new runs");
    assert!(status.success());

    // Full scaffold present.
    for expected in [
        "src/App.java",
        "src/HelloController.java",
        "src/HelloService.java",
        "rustjvm.toml",
        "pom.xml",
        "README.md",
    ] {
        assert!(project.join(expected).exists(), "missing {expected}");
    }

    let app = std::fs::read_to_string(project.join("src/App.java")).unwrap();
    assert!(app.contains("package com.example;"));
    assert!(app.contains("@RustJVMApplication"));
    assert!(app.contains("RustJVMBootstrap.run(App.class, args)"));

    // The real proof: the compiler discovers the scaffolded route.
    let out = rustjvm()
        .arg("routes")
        .arg("--src")
        .arg(project.join("src"))
        .output()
        .expect("rustjvm routes runs");
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert!(stdout.contains("/hello"), "route table:\n{stdout}");
    assert!(stdout.contains("HelloController.hello"), "route table:\n{stdout}");
}

#[test]
fn new_rejects_invalid_package() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("bad-app");

    let status = rustjvm()
        .arg("new")
        .arg(&project)
        .arg("--package")
        .arg("not a package!")
        .status()
        .expect("rustjvm new runs");
    assert!(!status.success());
    assert!(!project.join("src").exists());
}

#[test]
fn new_refuses_non_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let project = tmp.path().join("occupied");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("existing.txt"), "mine").unwrap();

    let status = rustjvm()
        .arg("new")
        .arg(&project)
        .status()
        .expect("rustjvm new runs");
    assert!(!status.success());
    // Existing content untouched.
    assert!(project.join("existing.txt").exists());
    assert!(!project.join("src").exists());
}
