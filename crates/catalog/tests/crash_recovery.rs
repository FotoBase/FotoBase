//! Integration test: killing the process mid-transaction must never corrupt the
//! catalog (TASK-01-005, PLAN §5 "SQLite robustness").

use fotobase_catalog::Catalog;
use tempfile::TempDir;

/// Child mode: the test binary re-executes itself with this env var set.
const CHILD_ENV: &str = "FB_CATALOG_CRASH_CHILD";

#[test]
fn kill9_mid_transaction_reopens_cleanly() {
    let dir = TempDir::new().unwrap();
    let db = dir.path().join("catalog.db");

    {
        let mut cat = Catalog::open(&db).expect("initial open");
        cat.seed_folder("2010/Beach").expect("seed folder");
    }

    // Child process: opens the catalog, starts a transaction, inserts rows, then
    // the parent SIGKILLs it mid-transaction.
    let exe = std::env::current_exe().unwrap();
    let mut child = std::process::Command::new(exe)
        .env(CHILD_ENV, db.to_string_lossy().into_owned())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("spawn crash child");

    // Give the child time to open the db and begin its transaction.
    std::thread::sleep(std::time::Duration::from_millis(500));
    child.kill().expect("SIGKILL the child mid-transaction");
    let _ = child.wait();

    // Reopen: must succeed, integrity must pass, uncommitted rows must be gone.
    let cat = Catalog::open(&db).expect("reopen after kill -9");
    cat.integrity_check().expect("integrity_check passes after kill -9");
    assert_eq!(cat.folder_count().expect("folder count"), 1, "only the committed seed remains");
}

#[test]
fn child_process_helper_is_registered() {
    // Marks this test binary as the crash-child runner; also a smoke check that
    // the child path opens, writes inside a transaction, and aborts on signal.
    if std::env::var(CHILD_ENV).is_err() {
        return;
    }
    let db = std::env::var(CHILD_ENV).unwrap();
    let mut cat = Catalog::open(&db).expect("child open");
    cat.begin_uncommitted_batch().expect("begin transaction");
    for i in 0..1000 {
        cat.seed_folder(&format!("2020/Trip{i}")).expect("child insert");
    }
    // Hold the transaction open; the parent SIGKILLs us here.
    loop {
        std::thread::sleep(std::time::Duration::from_secs(3600));
    }
}
