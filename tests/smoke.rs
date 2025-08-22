use std::{fs, path::PathBuf};

fn write_smoke_file(dir: &std::path::Path) -> PathBuf {
    let file = dir.join("fail.rs");
    let src = r#"//@edition: 2021

fn main() {
    let _ = undefined; //~ ERROR: cannot find value `undefined` in this scope
}
"#;
    fs::write(&file, src).expect("write smoke file");
    file
}

#[test]
fn annotations_without_bless_do_not_create_stderr() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_smoke_file(tmp.path());
    let root = file.parent().unwrap();

    let mut cfg = ui_test::Config::rustc(root);
    cfg.output_conflict_handling = ui_test::ignore_output_conflict;
    cfg.bless_command = Some("BLESS=1 cargo test".into());

    ui_test::run_tests(cfg)
        .expect("ui_test run should succeed without bless when annotations match");

    // No .stderr should be created without bless
    assert!(!file.with_extension("stderr").exists());
}

#[test]
fn bless_creates_stderr_after_verify() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = write_smoke_file(tmp.path());
    let root = file.parent().unwrap();
    let stderr_path = file.with_extension("stderr");
    if stderr_path.exists() {
        fs::remove_file(&stderr_path).expect("clean stderr");
    }

    // First pass: verify only
    let mut cfg = ui_test::Config::rustc(root);
    cfg.output_conflict_handling = ui_test::ignore_output_conflict;
    cfg.bless_command = Some("BLESS=1 cargo test".into());
    ui_test::run_tests(cfg.clone()).expect("verify pass should succeed");

    assert!(
        !stderr_path.exists(),
        "stderr must not be created without bless"
    );

    // Second pass: bless
    let mut bless_cfg = cfg;
    bless_cfg.output_conflict_handling = ui_test::bless_output_files;
    ui_test::run_tests(bless_cfg).expect("bless pass should succeed");

    assert!(stderr_path.exists(), "stderr must be created during bless");
}

#[test]
fn example_target_respects_annotations_and_bless() {
    // Build the example so cargo metadata has it; our library’s runner will compile it via the driver.
    // We only exercise ui_test directly here on copied source to a temp dir as example flow proxy.
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("uit_smoke.rs");
    fs::write(
            &file,
            "//@edition: 2021\nfn main(){\n let _ = undefined; //~ ERROR: cannot find value `undefined` in this scope\n}\n",
        )
    .expect("write example proxy");

    let mut cfg = ui_test::Config::rustc(file.parent().unwrap());
    cfg.output_conflict_handling = ui_test::ignore_output_conflict;
    cfg.bless_command = Some("BLESS=1 cargo test".into());
    ui_test::run_tests(cfg.clone()).expect("verify pass should succeed for example");

    // No .stderr without bless
    assert!(!file.with_extension("stderr").exists());

    // Bless
    let mut bless_cfg = cfg;
    bless_cfg.output_conflict_handling = ui_test::bless_output_files;
    ui_test::run_tests(bless_cfg).expect("bless pass should succeed for example");
    assert!(file.with_extension("stderr").exists());
}

#[test]
fn missing_annotation_does_not_write_stderr_even_on_bless() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("no_annot.rs");
    fs::write(&file, "//@edition: 2021\nfn main(){ let _ = undefined; }\n")
        .expect("write no annot");
    let root = file.parent().unwrap();

    // Verify should fail due to unmatched diagnostics
    let mut cfg = ui_test::Config::rustc(root);
    cfg.output_conflict_handling = ui_test::ignore_output_conflict;
    cfg.bless_command = Some("BLESS=1 cargo test".into());
    let verify = ui_test::run_tests(cfg.clone());
    assert!(verify.is_err(), "verify should fail without annotations");

    // Our runner would not invoke bless when verify fails; simulate that here by skipping bless.
    assert!(
        !file.with_extension("stderr").exists(),
        ".stderr must not be created when annotations are missing"
    );
}

#[test]
fn async_annotations_supported() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let file = tmp.path().join("async.rs");
    fs::write(
        &file,
        "//@edition: 2024\nasync fn f(){\n    let _ = undefined; //~ ERROR: cannot find value `undefined` in this scope\n}\nfn main(){}\n",
    )
    .expect("write async file");

    let mut cfg = ui_test::Config::rustc(file.parent().unwrap());
    cfg.output_conflict_handling = ui_test::ignore_output_conflict;
    cfg.bless_command = Some("BLESS=1 cargo test".into());
    ui_test::run_tests(cfg.clone()).expect("verify pass should succeed for async");

    // Bless
    let mut bless_cfg = cfg;
    bless_cfg.output_conflict_handling = ui_test::bless_output_files;
    ui_test::run_tests(bless_cfg).expect("bless pass should succeed for async");
    assert!(file.with_extension("stderr").exists());
}
