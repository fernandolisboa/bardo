//! Throwaway probe for #200; removed before merge.
#![cfg(windows)]

use std::collections::HashMap;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keyring_core::Entry;
use keyring_core::api::CredentialStoreApi as _;
use windows_native_keyring_store::Store;

fn entry(store: &Store, target: &str) -> Entry {
    let modifiers = HashMap::from([("target", target), ("persistence", "Local")]);
    store
        .build("app credentials", target, Some(&modifiers))
        .unwrap()
}

fn kind<T: std::fmt::Debug>(result: &keyring_core::Result<T>) -> String {
    match result {
        Ok(value) => format!("Ok({value:?})"),
        Err(keyring_core::Error::NoEntry) => "NoEntry".into(),
        Err(error) => format!("Err({error})"),
    }
}

fn text(result: keyring_core::Result<Vec<u8>>) -> keyring_core::Result<String> {
    result.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// One run of the contract's steps; an anomaly description when the
/// credential is still there after two deletes.
fn cycle(store: &Store, tag: &str, n: u32) -> Option<String> {
    let target = format!("BardoProbe/{}/app/youtube", uuid::Uuid::new_v4());
    let e = entry(store, &target);
    e.set_secret(format!("{tag}-{n}-one").as_bytes()).unwrap();
    let r0 = text(e.get_secret());
    e.set_secret(format!("{tag}-{n}-two").as_bytes()).unwrap();
    let r1 = text(e.get_secret());
    let d1 = e.delete_credential();
    let d2 = e.delete_credential();
    let r2 = text(e.get_secret());
    if matches!(r2, Err(keyring_core::Error::NoEntry)) {
        return None;
    }
    let d1k = kind(&d1);
    let d2k = kind(&d2);
    let r2k = kind(&r2);
    // How long does it stay?
    let r3 = kind(&text(entry(store, &target).get_secret()));
    std::thread::sleep(Duration::from_millis(50));
    let r4 = kind(&text(entry(store, &target).get_secret()));
    std::thread::sleep(Duration::from_millis(500));
    let r5 = kind(&text(entry(store, &target).get_secret()));
    let d3 = kind(&entry(store, &target).delete_credential());
    let r6 = kind(&text(entry(store, &target).get_secret()));
    let _ = entry(store, &target).delete_credential();
    Some(format!(
        "PROBE ANOMALY {tag}#{n}: r0={} r1={} d1={d1k} d2={d2k} after={r2k} again={r3} +50ms={r4} +550ms={r5} d3={d3} then={r6}",
        kind(&r0),
        kind(&r1)
    ))
}

fn disk_load(stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let dir = tempfile::tempdir().unwrap();
        let block = vec![7u8; 4 << 20];
        let mut i = 0u32;
        while !stop.load(Ordering::SeqCst) {
            let path = dir.path().join(format!("load-{}", i % 8));
            let mut file = std::fs::File::create(&path).unwrap();
            for _ in 0..8 {
                file.write_all(&block).unwrap();
            }
            file.sync_all().unwrap();
            i += 1;
        }
    })
}

#[test]
fn probe() {
    let mode = std::env::var("PROBE_MODE").unwrap_or_else(|_| "serial".into());
    let threads: u32 = if mode.starts_with("concurrent") { 8 } else { 1 };
    let cycles: u32 = std::env::var("PROBE_CYCLES")
        .ok()
        .and_then(|c| c.parse().ok())
        .unwrap_or(2400);
    let stop = Arc::new(AtomicBool::new(false));
    let loads: Vec<_> = if mode.ends_with("disk") {
        (0..4).map(|_| disk_load(Arc::clone(&stop))).collect()
    } else {
        Vec::new()
    };
    let store = Store::new().unwrap();
    let anomalies = Arc::new(Mutex::new(Vec::new()));
    let done = Arc::new(AtomicU32::new(0));
    let started = Instant::now();
    let workers: Vec<_> = (0..threads)
        .map(|t| {
            let store = Arc::clone(&store);
            let anomalies = Arc::clone(&anomalies);
            let done = Arc::clone(&done);
            std::thread::spawn(move || {
                for n in 0..cycles / threads {
                    if let Some(anomaly) = cycle(&store, &format!("t{t}"), n) {
                        println!("{anomaly}");
                        anomalies.lock().unwrap().push(anomaly);
                    }
                    done.fetch_add(1, Ordering::SeqCst);
                }
            })
        })
        .collect();
    for worker in workers {
        worker.join().unwrap();
    }
    stop.store(true, Ordering::SeqCst);
    for load in loads {
        load.join().unwrap();
    }
    println!(
        "PROBE mode={mode} threads={threads} cycles={} anomalies={} in {:?}",
        done.load(Ordering::SeqCst),
        anomalies.lock().unwrap().len(),
        started.elapsed()
    );
}
