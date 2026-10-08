use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use lumen_core::CancellationToken;

use crate::*;

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

fn sha(bytes: &[u8]) -> &'static str {
    use sha2::Digest;
    leak(
        sha2::Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}

/// A zip with deflated members (what wheels use).
fn zip(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in members {
        let mut enc =
            flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(data).unwrap();
        let comp = enc.finish().unwrap();
        let mut crc = crc32fast::Hasher::new();
        crc.update(data);
        let crc = crc.finalize();
        let offset = u32::try_from(out.len()).unwrap();
        let n = u16::try_from(name.len()).unwrap();
        let (csize, size) = (
            u32::try_from(comp.len()).unwrap(),
            u32::try_from(data.len()).unwrap(),
        );
        out.extend(0x0403_4b50u32.to_le_bytes());
        out.extend([20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
        out.extend(crc.to_le_bytes());
        out.extend(csize.to_le_bytes());
        out.extend(size.to_le_bytes());
        out.extend(n.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(name.as_bytes());
        out.extend(&comp);
        central.extend(0x0201_4b50u32.to_le_bytes());
        central.extend([20, 0, 20, 0, 0, 0, 8, 0, 0, 0, 0, 0]);
        central.extend(crc.to_le_bytes());
        central.extend(csize.to_le_bytes());
        central.extend(size.to_le_bytes());
        central.extend(n.to_le_bytes());
        central.extend([0u8; 12]);
        central.extend(offset.to_le_bytes());
        central.extend(name.as_bytes());
    }
    let cd_offset = u32::try_from(out.len()).unwrap();
    let cd_size = u32::try_from(central.len()).unwrap();
    out.extend(&central);
    let count = u16::try_from(members.len()).unwrap();
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend(count.to_le_bytes());
    out.extend(count.to_le_bytes());
    out.extend(cd_size.to_le_bytes());
    out.extend(cd_offset.to_le_bytes());
    out.extend([0u8; 2]);
    out
}

struct Tmp(PathBuf);

impl Tmp {
    fn new(tag: &str) -> Self {
        let p = std::env::temp_dir().join(format!("lumen-provision-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("mirror")).unwrap();
        Self(p)
    }
    fn mirror(&self) -> PathBuf {
        self.0.join("mirror")
    }
    fn root(&self) -> PathBuf {
        self.0.join("root")
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A component of two plain files and one archive, mirrored into `t`.
fn component(t: &Tmp, corrupt_second: bool) -> Component {
    let a = b"tokenizer contents".to_vec();
    let b = vec![7u8; 300_000];
    let lib = b"fake runtime library".to_vec();
    let notice = b"MIT License".to_vec();
    let wheel = zip(&[
        ("pkg/capi/rt.dll", &lib),
        ("pkg/LICENSE", &notice),
        ("pkg/other.py", b"x"),
    ]);
    std::fs::write(t.mirror().join("a.json"), &a).unwrap();
    let mut stored_b = b.clone();
    if corrupt_second {
        stored_b[10] ^= 1;
    }
    std::fs::write(t.mirror().join("b.bin"), &stored_b).unwrap();
    std::fs::write(t.mirror().join("rt.whl"), &wheel).unwrap();
    let files: &'static [RemoteFile] = Box::leak(Box::new([
        RemoteFile {
            url: "https://example.test/r/a.json",
            size: a.len() as u64,
            sha256: sha(&a),
            install: Install::As("a.json"),
        },
        RemoteFile {
            url: "https://example.test/r/b.bin",
            size: b.len() as u64,
            sha256: sha(&b),
            install: Install::As("onnx/b.bin"),
        },
        RemoteFile {
            url: "https://example.test/r/rt.whl",
            size: wheel.len() as u64,
            sha256: sha(&wheel),
            install: Install::Extract(Box::leak(Box::new([
                Member {
                    name: "pkg/capi/rt.dll",
                    dest: "rt.dll",
                    size: lib.len() as u64,
                    sha256: sha(&lib),
                },
                Member {
                    name: "pkg/LICENSE",
                    dest: "LICENSE",
                    size: notice.len() as u64,
                    sha256: sha(&notice),
                },
            ]))),
        },
    ]));
    Component {
        id: "models/test",
        version: "v1",
        title: "test",
        license: "MIT",
        license_url: "https://example.test",
        host: "example.test",
        files,
        notices: &["LICENSE"],
        platform_ok: true,
    }
}

/// Counts calls; can cancel after `stop_after` files.
struct Counting {
    inner: DirFetch,
    calls: Mutex<Vec<String>>,
    stop_after: Option<usize>,
    cancel: CancellationToken,
}

impl Fetch for Counting {
    fn fetch(
        &self,
        url: &str,
        dest: &Path,
        cancel: &CancellationToken,
        progress: &mut dyn FnMut(u64),
    ) -> Result<(), FetchError> {
        let mut calls = self.calls.lock().unwrap();
        if self.stop_after.is_some_and(|n| calls.len() >= n) {
            self.cancel.cancel();
            return Err(FetchError::Cancelled);
        }
        calls.push(fetch::url_file_name(url).to_owned());
        drop(calls);
        self.inner.fetch(url, dest, cancel, progress)
    }
}

#[test]
fn installs_verifies_and_removes() {
    let t = Tmp::new("ok");
    let c = component(&t, false);
    let mut seen = Vec::new();
    let dir = install(
        &t.root(),
        &c,
        &DirFetch { dir: t.mirror() },
        &CancellationToken::new(),
        &mut |p| seen.push(p),
    )
    .unwrap();
    assert_eq!(dir, component_dir(&t.root(), &c));
    assert!(seen.windows(2).all(|w| w[0].done <= w[1].done));
    assert_eq!(seen.last().map(|p| p.done), Some(c.download_bytes()));
    assert_eq!(
        std::fs::read(dir.join("rt.dll")).unwrap(),
        b"fake runtime library"
    );
    assert!(dir.join("onnx").join("b.bin").exists() && dir.join("LICENSE").exists());
    assert!(
        !dir.join("other.py").exists(),
        "only listed members are extracted"
    );
    assert_eq!(state(&t.root(), &c), State::Installed { dir: dir.clone() });
    assert!(verify(&t.root(), &c).unwrap().is_empty());
    // Tampering is caught by verify.
    std::fs::write(dir.join("a.json"), b"tokenizer CONTENTS").unwrap();
    assert_eq!(verify(&t.root(), &c).unwrap(), ["a.json"]);
    remove(&t.root(), &c).unwrap();
    assert_eq!(state(&t.root(), &c), State::NotInstalled);
    assert_eq!(c.installed_bytes(), 18 + 300_000 + 20 + 11);
}

#[test]
fn a_corrupt_download_is_rejected_and_deleted() {
    let t = Tmp::new("corrupt");
    let c = component(&t, true);
    let err = install(
        &t.root(),
        &c,
        &DirFetch { dir: t.mirror() },
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(
        matches!(err, InstallError::Corrupt(ref f) if f == "b.bin"),
        "{err}"
    );
    assert!(matches!(state(&t.root(), &c), State::Partial { .. }));
    let download = t
        .root()
        .join("models")
        .join("test")
        .join("v1.partial")
        .join(".download")
        .join("b.bin");
    assert!(!download.exists());
}

#[test]
fn a_cancelled_install_resumes_without_refetching() {
    let t = Tmp::new("resume");
    let c = component(&t, false);
    let first = Counting {
        inner: DirFetch { dir: t.mirror() },
        calls: Mutex::new(Vec::new()),
        stop_after: Some(1),
        cancel: CancellationToken::new(),
    };
    let err = install(&t.root(), &c, &first, &first.cancel.clone(), &mut |_| {}).unwrap_err();
    assert!(matches!(err, InstallError::Cancelled));
    assert!(matches!(state(&t.root(), &c), State::Partial { bytes } if bytes > 0));
    let second = Counting {
        inner: DirFetch { dir: t.mirror() },
        calls: Mutex::new(Vec::new()),
        stop_after: None,
        cancel: CancellationToken::new(),
    };
    install(
        &t.root(),
        &c,
        &second,
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(*second.calls.lock().unwrap(), ["b.bin", "rt.whl"]);
    // Installing again is a no-op.
    install(
        &t.root(),
        &c,
        &second,
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(second.calls.lock().unwrap().len(), 2);
}

#[test]
fn unsupported_platforms_and_pinned_manifests() {
    let t = Tmp::new("platform");
    let mut c = component(&t, false);
    c.platform_ok = false;
    assert!(matches!(
        install(
            &t.root(),
            &c,
            &DirFetch { dir: t.mirror() },
            &CancellationToken::new(),
            &mut |_| {}
        ),
        Err(InstallError::Unsupported)
    ));
    for comp in [EMBEDDING_MODEL, INFERENCE_RUNTIME] {
        for f in comp.files {
            assert!(f.url.starts_with("https://"));
            assert_eq!(f.sha256.len(), 64);
            assert!(
                f.sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            );
        }
    }
    assert_eq!(EMBEDDING_MODEL.download_bytes(), 206_718_772);
    assert_eq!(INFERENCE_RUNTIME.download_bytes(), 14_311_470);
    assert!(
        EMBEDDING_MODEL.files[2]
            .url
            .contains("/daa72c51243991dfcaf9f9137d2c573d8f7790c0/onnx/")
    );
}

/// Real HTTPS download of the pinned runtime wheel through the system curl (14 MB).
/// `cargo test -p lumen-provision -- --ignored`
#[test]
#[ignore = "network"]
fn real_download_of_the_runtime_wheel() {
    let t = Tmp::new("net");
    let curl = CurlFetch::new();
    assert!(curl.available());
    let c = Component {
        platform_ok: true,
        ..INFERENCE_RUNTIME
    };
    let mut last = None;
    let dir = install(&t.root(), &c, &curl, &CancellationToken::new(), &mut |p| {
        last = Some(p)
    })
    .unwrap();
    assert!(verify(&t.root(), &c).unwrap().is_empty());
    assert_eq!(last.map(|p| p.done), Some(c.download_bytes()));
    assert!(dir.join("onnxruntime.dll").exists() && dir.join("ThirdPartyNotices.txt").exists());
}

/// Real HTTPS download of the pinned model (207 MB) through the system curl.
/// `cargo test -p lumen-provision -- --ignored real_download_of_the_model`
#[test]
#[ignore = "network, 207 MB"]
fn real_download_of_the_model() {
    let t = Tmp::new("net-model");
    let dir = install(
        &t.root(),
        &EMBEDDING_MODEL,
        &CurlFetch::new(),
        &CancellationToken::new(),
        &mut |_| {},
    )
    .unwrap();
    assert!(dir.join("onnx").join("model_q4.onnx_data").exists());
    assert!(verify(&t.root(), &EMBEDDING_MODEL).unwrap().is_empty());
}
