//! One explicitly selected local text file, captured once; a receipt never authorizes sending.
use base64::{Engine as _, engine::general_purpose::STANDARD};
use hermes_tern_core::{
    error::{Error, Result},
    text::is_bidi_control,
};
use serde_json::{Value, json};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
};

const MAX_FILE_BYTES: usize = 256 * 1024;
const MAX_PATH_BYTES: usize = 4096;
const MAX_NAME_BYTES: usize = 255;

fn safe_text(value: &str) -> bool {
    !value
        .chars()
        .any(|c| c.is_control() || is_bidi_control(c) || matches!(c, '\u{2028}' | '\u{2029}'))
}

fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_BYTES
        && name.trim() == name
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\'])
        && safe_text(name)
}

fn unchanged(before: &Metadata, after: &Metadata) -> bool {
    before.is_file()
        && after.is_file()
        && before.dev() == after.dev()
        && before.ino() == after.ino()
        && before.len() == after.len()
        && before.mtime() == after.mtime()
        && before.mtime_nsec() == after.mtime_nsec()
        && before.ctime() == after.ctime()
        && before.ctime_nsec() == after.ctime_nsec()
}

fn known_binary(bytes: &[u8]) -> bool {
    [
        &b"%PDF-"[..],
        &b"\x89PNG\r\n\x1a\n"[..],
        &b"\xff\xd8\xff"[..],
        &b"GIF87a"[..],
        &b"GIF89a"[..],
        &b"BM"[..],
        &b"PK\x03\x04"[..],
        &b"PK\x05\x06"[..],
        &b"PK\x07\x08"[..],
        &b"\x1f\x8b"[..],
        &b"ID3"[..],
    ]
    .iter()
    .any(|signature| bytes.starts_with(signature))
        || (bytes.starts_with(b"RIFF")
            && (bytes.get(8..12) == Some(&b"WEBP"[..]) || bytes.get(8..12) == Some(&b"WAVE"[..])))
        || bytes.get(4..8) == Some(&b"ftyp"[..])
}

/// Private bounded byte snapshot, not a reference to future filesystem contents.
/// No `Clone` or `Debug`: neither the payload nor its base64 representation is logged.
pub struct PreparedFile {
    id: String,
    path: String,
    name: String,
    bytes: Vec<u8>,
}

impl PreparedFile {
    /// Read exactly one canonical UTF-8 absolute path. No expansion, rewriting or repair.
    /// NOFOLLOW protects the final component; this is not a same-UID adversarial filesystem sandbox.
    pub fn read(path: &str) -> Result<Self> {
        let public = Path::new(path);
        if path.is_empty()
            || path.len() > MAX_PATH_BYTES
            || !public.is_absolute()
            || !safe_text(path)
        {
            return Err(Error::Protocol("unsafe attachment path"));
        }
        let name = public
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(Error::Protocol("unsafe attachment name"))?;
        if !safe_name(name) || fs::canonicalize(public)?.to_str() != Some(path) {
            return Err(Error::Protocol("noncanonical attachment path or name"));
        }
        let binary_extension = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| {
                matches!(
                    ext.to_ascii_lowercase().as_str(),
                    "pdf"
                        | "png"
                        | "jpg"
                        | "jpeg"
                        | "gif"
                        | "webp"
                        | "bmp"
                        | "zip"
                        | "gz"
                        | "mp3"
                        | "mp4"
                        | "wav"
                )
            });
        if binary_extension {
            return Err(Error::Protocol("binary attachment extension"));
        }
        let before_path = fs::symlink_metadata(public)?;
        if !before_path.is_file() {
            return Err(Error::Protocol("attachment must be a regular file"));
        }
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(public)?;
        let before = file.metadata()?;
        if !unchanged(&before_path, &before) || before.len() > MAX_FILE_BYTES as u64 {
            return Err(Error::Protocol("attachment changed or exceeds byte cap"));
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        let after = file.metadata()?;
        let after_path = fs::symlink_metadata(public)?;
        if bytes.len() > MAX_FILE_BYTES
            || bytes.len() as u64 != before.len()
            || !unchanged(&before, &after)
            || !unchanged(&before, &after_path)
            || fs::canonicalize(public)?.to_str() != Some(path)
        {
            return Err(Error::Protocol("attachment changed during bounded read"));
        }
        if bytes.contains(&0) || known_binary(&bytes) || std::str::from_utf8(&bytes).is_err() {
            return Err(Error::Protocol("attachment is not supported UTF-8 text"));
        }
        Ok(Self {
            id: uuid::Uuid::new_v4().to_string(),
            path: path.to_owned(),
            name: name.to_owned(),
            bytes,
        })
    }

    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    /// Construct the upload from the immutable snapshot; never include the local path.
    pub fn params(&self, session_id: &str) -> Value {
        json!({
            "session_id": session_id,
            "data_url": format!("data:text/plain;base64,{}", STANDARD.encode(&self.bytes)),
            "name": self.name,
        })
    }
}

fn safe_ref_path(value: &str) -> bool {
    if value.is_empty()
        || value.len() > MAX_PATH_BYTES
        || value.trim() != value
        || !safe_text(value)
        || value.contains('\\')
        || value.starts_with('~')
    {
        return false;
    }
    let rest = value.strip_prefix('/').unwrap_or(value);
    !rest.is_empty()
        && rest
            .split('/')
            .all(|component| !component.is_empty() && component != "." && component != "..")
}

/// Pinned Hermes `tui_gateway/prompt_attachments.py` `_format_ref_value` ordering.
fn format_ref_value(value: &str) -> Option<String> {
    let needs_quote = value
        .chars()
        .any(|c| c.is_whitespace() || "()[]{}<>\"'`".contains(c));
    if !needs_quote {
        return Some(value.to_owned());
    }
    for quote in ['`', '"', '\''] {
        if !value.contains(quote) {
            return Some(format!("{quote}{value}{quote}"));
        }
    }
    None
}

/// Only fields required for a validated, bounded draft reference survive projection.
pub struct AttachmentReceipt {
    name: String,
    ref_path: String,
    reference: String,
}

impl AttachmentReceipt {
    pub fn parse(value: Value) -> Result<Self> {
        let fields = value
            .as_object()
            .ok_or(Error::Protocol("invalid attachment receipt"))?;
        if fields.get("attached") != Some(&Value::Bool(true))
            || fields.get("uploaded") != Some(&Value::Bool(true))
        {
            return Err(Error::Protocol("attachment was not uploaded"));
        }
        let name = fields
            .get("name")
            .and_then(Value::as_str)
            .ok_or(Error::Protocol("missing attachment name"))?;
        let ref_path = fields
            .get("ref_path")
            .and_then(Value::as_str)
            .ok_or(Error::Protocol("missing attachment reference path"))?;
        let ref_text = fields
            .get("ref_text")
            .and_then(Value::as_str)
            .ok_or(Error::Protocol("missing attachment reference"))?;
        if !safe_name(name) || !safe_ref_path(ref_path) || ref_path.rsplit('/').next() != Some(name)
        {
            return Err(Error::Protocol("unsafe attachment reference path or name"));
        }
        let formatted = format_ref_value(ref_path)
            .ok_or(Error::Protocol("ambiguous attachment reference quoting"))?;
        if formatted == ref_path {
            // The owner's formatter does not quote these, but its submit parser
            // strips bare punctuation and interprets :N[-M] as a line range.
            let digits = |s: &str| !s.is_empty() && s.chars().all(char::is_numeric);
            let line_range = ref_path.rsplit_once(':').is_some_and(|(_, suffix)| {
                digits(suffix)
                    || suffix
                        .split_once('-')
                        .is_some_and(|(a, b)| digits(a) && digits(b))
            });
            if ref_path.ends_with([',', '.', ';', '!', '?']) || line_range {
                return Err(Error::Protocol(
                    "attachment reference does not name one exact full file",
                ));
            }
        }
        let expected = format!("@file:{formatted}");
        if expected.len() > MAX_PATH_BYTES + 8 || ref_text != expected {
            return Err(Error::Protocol("invalid attachment reference text"));
        }
        Ok(Self {
            name: name.to_owned(),
            ref_path: ref_path.to_owned(),
            reference: expected,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn ref_path(&self) -> &str {
        &self.ref_path
    }
    pub fn reference(&self) -> &str {
        &self.reference
    }
}
