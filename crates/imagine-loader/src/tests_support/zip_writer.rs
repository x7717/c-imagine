//! In-memory jar builder for tests: real zip bytes, no filesystem.

/// Build a jar (zip) from flat entries and raw nested payloads.
///
/// * `files` — `(path, content)` pairs stored uncompressed-ish via deflate.
/// * `nested` — `(path, payload)` pairs where payload is arbitrary bytes
///   (e.g. a nested jar built by this helper) stored as another entry.
pub fn build_jar(files: &[(&str, &[u8])], nested: &[(&str, &[u8])]) -> Vec<u8> {
    use std::io::Write as _;
    use zip::ZipWriter;

    let buf = std::io::Cursor::new(Vec::new());
    let mut w = ZipWriter::new(buf);
    let opts = zip::write::SimpleFileOptions::default();
    for (path, data) in files.iter().chain(nested.iter()) {
        w.start_file(*path, opts).expect("start_file");
        w.write_all(data).expect("write entry");
    }
    w.finish().expect("finish zip").into_inner()
}
