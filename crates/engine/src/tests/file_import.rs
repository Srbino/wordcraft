use super::*;

#[test]
fn native_managed_import_preserves_identity_is_dirty_and_undoable() {
    let mut s = Session::new(wordcraft_doc::Document::from_text("existing server document"));
    let id = s.document_id();
    let source = wordcraft_doc::Document::from_text("imported document body");
    let bytes = crate::io::save_bytes("source.docx", &source).unwrap();
    s.run("file.import", &json!({"path":"source.docx","data":crate::cmd::insert::base64_encode(&bytes)})).unwrap();
    assert_eq!(s.document_id(), id);
    assert_eq!(s.documents().len(), 1);
    assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "imported document body");
    assert!(s.dirty);
    s.run("edit.undo", &json!({})).unwrap();
    assert_eq!(s.document_id(), id);
    assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "existing server document");
    s.run("edit.redo", &json!({})).unwrap();
    assert_eq!(s.doc.plain_text(wordcraft_doc::StoryRef::Body), "imported document body");
}

#[test]
fn native_managed_import_is_atomic_and_refuses_document_protection() {
    let mut s = Session::new(wordcraft_doc::Document::from_text("private existing body"));
    let before = serde_json::to_value(&s.doc).unwrap();
    let id = s.document_id();
    assert!(s.run("file.import", &json!({"path":"bad.docx","data":"bm90IGEgd29yZCBkb2N1bWVudA=="})).is_err());
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
    s.run("review.restrict", &json!({"mode":"readOnly"})).unwrap();
    let before = serde_json::to_value(&s.doc).unwrap();
    let bytes = crate::io::save_bytes("copy.docx", &wordcraft_doc::Document::from_text("forbidden import")).unwrap();
    assert!(s.run("file.import", &json!({"path":"copy.docx","data":crate::cmd::insert::base64_encode(&bytes)})).is_err());
    assert_eq!(s.document_id(), id);
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), before);
}

#[test]
fn native_managed_import_retains_the_whole_rich_native_document() {
    let mut source = Session::new(crate::sample::sample_document());
    run(&mut source, "review.newComment", json!({"text":"authored comment"}));
    source.sel = crate::Selection::caret(Pos::body(0, 0));
    run(&mut source, "insert.header", json!({"text":"authored header"}));
    source.sel = crate::Selection::caret(Pos::body(0, 0));
    run(&mut source, "insert.footer", json!({"text":"authored footer"}));
    source.sel = crate::Selection::caret(Pos::body(0, 0));
    run(
        &mut source,
        "insert.picture",
        json!({"data":"iVBORw0KGgoAAAANSUhEUgAAACAAAAAQCAYAAAB3AH1ZAAAAKElEQVR4nGMQibrznxJ8R0ODIsww6oBRB4w6YNQBow4YdcCoAwbaAQCfenQ9EOdSfAAAAABJRU5ErkJggg=="}),
    );
    let bytes = crate::io::save_bytes("rich.json", &source.doc).unwrap();
    let expected = serde_json::to_value(&source.doc).unwrap();
    let mut s = s();
    let id = s.document_id();
    run(&mut s, "file.import", json!({"path":"rich.json","data":crate::cmd::insert::base64_encode(&bytes)}));
    assert_eq!(s.document_id(), id);
    assert_eq!(serde_json::to_value(&s.doc).unwrap(), expected);
    assert!(s.dirty);
}
