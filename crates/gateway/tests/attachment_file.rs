use hermes_tern_gateway::attachment_file::{AttachmentReceipt, PreparedFile};
use serde_json::{Value, json};
use std::{
    fs::{self, DirBuilder},
    os::unix::fs::{DirBuilderExt, symlink},
    path::{Path, PathBuf},
};

// Every source file in these tests is created within this test's private TMPDIR child.
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        let base = PathBuf::from(std::env::var_os("TMPDIR").expect("private TMPDIR required"));
        assert!(base.is_absolute());
        let path = base.join(format!("hermes-tern-attachment-{}", uuid::Uuid::new_v4()));
        DirBuilder::new().mode(0o700).create(&path).unwrap();
        fs::write(
            path.join("ownership.json"),
            serde_json::to_vec(&json!({
                "retention":"retain;no cleanup", "paths":[path,path.join("ownership.json")]
            }))
            .unwrap(),
        )
        .unwrap();
        Self(path)
    }
    fn file(&self, name: &str, bytes: &[u8]) -> String {
        let path = self.0.join(name);
        fs::write(&path, bytes).unwrap();
        path.into_os_string().into_string().unwrap()
    }
    fn path(&self, name: &str) -> String {
        self.0.join(name).into_os_string().into_string().unwrap()
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        // Retain only this test's exact private entries; never follow alias symlinks
        // or recursively delete a directory while another probe may still hold it.
        let paths = std::iter::once(self.0.clone())
            .chain(
                fs::read_dir(&self.0)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            )
            .collect::<Vec<_>>();
        fs::write(
            self.0.join("ownership.json"),
            serde_json::to_vec(&json!({
                "retention":"retain;no cleanup", "paths":paths
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
fn receipt(name: &str, path: &str, reference: &str) -> Value {
    json!({"attached":true,"uploaded":true,"name":name,"ref_path":path,"ref_text":reference})
}

#[test]
fn snapshot_is_exact_immutable_text_and_params_have_no_path() {
    let scratch = Scratch::new();
    let path = scratch.file("té st.txt", b"first\nline");
    let prepared = PreparedFile::read(&path).unwrap();
    assert_eq!(prepared.path(), path);
    assert_eq!(prepared.name(), "té st.txt");
    assert_eq!(prepared.len(), 10);
    assert!(!prepared.is_empty());
    assert_eq!(
        uuid::Uuid::parse_str(prepared.id())
            .unwrap()
            .get_version_num(),
        4
    );
    assert_ne!(prepared.id(), PreparedFile::read(&path).unwrap().id());
    fs::write(&path, b"replaced").unwrap();
    assert_eq!(
        prepared.params("session-A"),
        json!({
            "session_id":"session-A", "name":"té st.txt",
            "data_url":"data:text/plain;base64,Zmlyc3QKbGluZQ=="
        })
    );
}

#[test]
fn empty_text_and_exact_cap_are_accepted_but_one_more_byte_is_rejected() {
    let scratch = Scratch::new();
    let empty = scratch.file("empty.txt", b"");
    let file = PreparedFile::read(&empty).unwrap();
    assert!(file.is_empty());
    assert_eq!(file.params("s")["data_url"], "data:text/plain;base64,");
    let exact = scratch.file("exact.txt", &vec![b'x'; 256 * 1024]);
    assert_eq!(PreparedFile::read(&exact).unwrap().len(), 256 * 1024);
    let over = scratch.file("over.txt", &vec![b'x'; 256 * 1024 + 1]);
    assert!(PreparedFile::read(&over).is_err());
}

#[test]
fn file_path_requires_literal_absolute_utf8_canonical_path_and_safe_basename() {
    let scratch = Scratch::new();
    let path = scratch.file("valid.txt", b"text");
    assert!(PreparedFile::read("valid.txt").is_err());
    assert!(PreparedFile::read(&path.replace("/valid.txt", "/./valid.txt")).is_err());
    assert!(PreparedFile::read(&path.replace("/valid.txt", "/../valid.txt")).is_err());
    assert!(PreparedFile::read(&path.replace("/valid.txt", "//valid.txt")).is_err());
    assert!(PreparedFile::read(&(path.clone() + " ")).is_err());
    assert!(PreparedFile::read(&(path.clone() + "\n")).is_err());
    assert!(PreparedFile::read(&(path.clone() + "\u{202e}")).is_err());
    assert!(PreparedFile::read(&(path.clone() + "\u{2028}")).is_err());
    assert!(PreparedFile::read(&format!("/{:04096}", "x")).is_err());
    let max_name = format!("{}.txt", "x".repeat(251));
    assert_eq!(
        PreparedFile::read(&scratch.file(&max_name, b"text"))
            .unwrap()
            .name(),
        max_name
    );
    let long_name = format!("{}.txt", "x".repeat(252));
    let long = scratch.path(&long_name);
    assert!(PreparedFile::read(&long).is_err());
    let bidi = scratch.file("bad\u{2066}.txt", b"text");
    assert!(PreparedFile::read(&bidi).is_err());
    let control = scratch.file("bad\n.txt", b"text");
    assert!(PreparedFile::read(&control).is_err());
}

#[test]
fn final_symlink_ancestor_symlink_directory_and_fifo_are_rejected_without_blocking() {
    let scratch = Scratch::new();
    let path = scratch.file("file.txt", b"text");
    symlink(&path, scratch.path("link.txt")).unwrap();
    assert!(PreparedFile::read(&scratch.path("link.txt")).is_err());
    symlink(&scratch.0, scratch.path("alias")).unwrap();
    assert!(PreparedFile::read(&scratch.path("alias/file.txt")).is_err());
    assert!(PreparedFile::read(scratch.0.to_str().unwrap()).is_err());
    let fifo = scratch.path("pipe.txt");
    assert!(
        std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    assert!(PreparedFile::read(&fifo).is_err());
}

#[test]
fn invalid_utf8_nul_binary_extensions_and_binary_magic_are_rejected() {
    let scratch = Scratch::new();
    for (n, body) in [
        ("invalid.txt", &b"ok\xff"[..]),
        ("nul.txt", &b"hello\0world"[..]),
        ("pdf.PDF", &b"ordinary UTF8"[..]),
        ("png.png", &b"ordinary UTF8"[..]),
        ("jpg.jpg", &b"ordinary UTF8"[..]),
        ("jpeg.jpeg", &b"ordinary UTF8"[..]),
        ("gif.gif", &b"ordinary UTF8"[..]),
        ("webp.webp", &b"ordinary UTF8"[..]),
        ("bmp.bmp", &b"ordinary UTF8"[..]),
        ("zip.zip", &b"ordinary UTF8"[..]),
        ("gz.gz", &b"ordinary UTF8"[..]),
        ("mp3.mp3", &b"ordinary UTF8"[..]),
        ("mp4.mp4", &b"ordinary UTF8"[..]),
        ("wav.wav", &b"ordinary UTF8"[..]),
        ("magic.txt", &b"%PDF-1.7\n"[..]),
        ("gif_magic.txt", &b"GIF89a bytes"[..]),
        ("id3_magic.txt", &b"ID3 ordinary"[..]),
    ] {
        let path = scratch.file(n, body);
        assert!(PreparedFile::read(&path).is_err(), "accepted {n}");
    }
    let utf8 = scratch.file("עברית.txt", "こんにちは\n".as_bytes());
    assert_eq!(
        PreparedFile::read(&utf8).unwrap().len(),
        "こんにちは\n".len()
    );
}

#[test]
fn receipt_projection_preserves_exact_safe_path_and_upstream_quote_priority() {
    for (path, expected) in [
        (
            ".hermes/attachments/hello.txt",
            "@file:.hermes/attachments/hello.txt",
        ),
        ("/home/user/my file.txt", "@file:`/home/user/my file.txt`"),
        ("hi`there.txt", "@file:\"hi`there.txt\""),
        ("hi`\"there.txt", "@file:'hi`\"there.txt'"),
        ("日本語/文書.txt", "@file:日本語/文書.txt"),
        ("paren(1).txt", "@file:`paren(1).txt`"),
    ] {
        let name = Path::new(path).file_name().unwrap().to_str().unwrap();
        let parsed = AttachmentReceipt::parse(receipt(name, path, expected)).unwrap();
        assert_eq!(parsed.name(), name);
        assert_eq!(parsed.ref_path(), path);
        assert_eq!(parsed.reference(), expected);
    }
}

#[test]
fn receipt_requires_explicit_truths_strings_exact_ref_and_basename_identity() {
    for value in [
        json!({}),
        json!(null),
        json!(false),
        json!([]),
        json!({"attached":true,"name":"file.txt","ref_path":"file.txt","ref_text":"@file:file.txt"}),
        json!({"attached":1,"uploaded":true,"name":"file.txt","ref_path":"file.txt","ref_text":"@file:file.txt"}),
        json!({"attached":true,"uploaded":"true","name":"file.txt","ref_path":"file.txt","ref_text":"@file:file.txt"}),
        json!({"attached":false,"uploaded":true,"name":"file.txt","ref_path":"file.txt","ref_text":"@file:file.txt"}),
        receipt("other.txt", "file.txt", "@file:file.txt"),
        receipt("file.txt", "file.txt", "@file:`file.txt`"),
        receipt("file.txt", "file.txt", "@file:file.txt\nSEND"),
        receipt("file.txt", "file.txt", "file.txt"),
        receipt("file.txt", "file.txt", "@file:file.txt\u{202e}"),
    ] {
        assert!(
            AttachmentReceipt::parse(value.clone()).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn receipt_rejects_ambiguous_paths_controls_and_all_three_quote_forms() {
    for path in [
        "",
        "/",
        "../a.txt",
        "./a.txt",
        "a/../b.txt",
        "/a/./b.txt",
        "/a//b.txt",
        "a/",
        "a\\b.txt",
        " a.txt",
        "a.txt\n",
        "bad\u{2028}.txt",
        "bad\u{2069}.txt",
        "bad`\"'.txt",
        &"x".repeat(4097),
    ] {
        let name = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("a.txt");
        assert!(
            AttachmentReceipt::parse(receipt(name, path, &format!("@file:{path}"))).is_err(),
            "accepted {path:?}"
        );
    }
    for name in [
        "",
        " ",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a\n.txt",
        "a\u{2029}.txt",
        &"x".repeat(256),
    ] {
        assert!(AttachmentReceipt::parse(receipt(name, "x.txt", "@file:x.txt")).is_err());
    }
}

#[test]
fn receipt_ignores_untrusted_raw_path_config_error_credentials_fields() {
    let mut data = receipt(
        "safe.txt",
        ".hermes/attachments/safe.txt",
        "@file:.hermes/attachments/safe.txt",
    );
    let obj = data.as_object_mut().unwrap();
    obj.insert("path".into(), json!("/owner/secrets"));
    obj.insert("config".into(), json!({"secret":"token"}));
    obj.insert("error".into(), json!("injected"));
    obj.insert("credentials".into(), json!("private"));
    let parsed = AttachmentReceipt::parse(data).unwrap();
    assert_eq!(parsed.reference(), "@file:.hermes/attachments/safe.txt");
    assert_eq!(parsed.ref_path(), ".hermes/attachments/safe.txt");
}

#[test]
fn accepted_reference_must_roundtrip_as_one_exact_full_file_not_punctuation_or_line_range() {
    for path in [
        "attachments/a.txt:1",
        "attachments/a.txt:1-2",
        "attachments/a.txt,",
        "attachments/a.txt;",
        "attachments/a.txt.",
        "attachments/a.txt!",
        "attachments/a.txt?",
        "~someone/file.txt",
        "~/file.txt",
    ] {
        let name = path.rsplit('/').next().unwrap();
        assert!(
            AttachmentReceipt::parse(receipt(name, path, &format!("@file:{path}"))).is_err(),
            "reference does not roundtrip in the pinned parser: {path}"
        );
    }
    // A quoted complete path does not acquire a line range or lose punctuation.
    for path in ["attachments/a file.txt:1", "attachments/a file.txt,"] {
        let name = path.rsplit('/').next().unwrap();
        assert!(AttachmentReceipt::parse(receipt(name, path, &format!("@file:`{path}`"))).is_ok());
    }
}
