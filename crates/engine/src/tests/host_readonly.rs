use super::*;

fn native(s: &Session) -> Vec<u8> {
    serde_json::to_vec(&s.doc).unwrap()
}

#[test]
fn native_host_readonly_is_ephemeral_and_blocks_all_registered_mutators() {
    let mut s = s();
    run(&mut s, "text.insert", json!({"text":"retained native text"}));
    let bytes = native(&s);
    let revision = s.rev();
    let dirty = s.dirty;
    assert!(!s.host_read_only());
    s.set_host_read_only(true);
    assert!(s.host_read_only());
    let ids: Vec<_> = s.registry.all().iter().filter(|spec| spec.mutates).map(|spec| spec.id).collect();
    assert!(ids.len() > 100);
    for id in ids {
        assert!(!s.host_command_allowed(id), "{id}");
        let error = s.run(id, &json!({})).unwrap_err().to_string();
        assert!(error.contains("read-only"), "{id}: {error}");
        assert_eq!(native(&s), bytes, "{id}");
        assert_eq!(s.rev(), revision, "{id}");
        assert_eq!(s.dirty, dirty, "{id}");
    }
    s.set_host_read_only(false);
    assert_eq!(native(&s), bytes);
    assert_eq!(s.rev(), revision);
    assert_eq!(s.dirty, dirty);
    run(&mut s, "text.insert", json!({"text":" enabled"}));
    assert_ne!(native(&s), bytes);
}

#[test]
fn native_host_readonly_also_blocks_pure_document_and_file_authority() {
    let mut s = s();
    run(&mut s, "text.insert", json!({"text":"retained"}));
    s.dirty = false;
    s.set_host_read_only(true);
    let bytes = native(&s);
    let revision = s.rev();
    let document = s.document_id();
    for id in [
        "file.new",
        "file.open",
        "file.save",
        "file.saveAs",
        "file.rename",
        "file.encrypt",
        "file.close",
        "file.closeDocument",
        "file.options",
        "file.recover",
        "file.newFromTemplate",
        "edit.undo",
        "edit.undoTo",
        "edit.redo",
        "edit.repeat",
        "insert.closeHeader",
        "tools.macros",
        "equation.exit",
    ] {
        assert!(s.registry.get(id).is_some(), "missing fixture command {id}");
        assert!(!s.host_command_allowed(id), "{id}");
        assert!(s.run(id, &json!({})).unwrap_err().to_string().contains("read-only"), "{id}");
        assert_eq!(native(&s), bytes, "{id}");
        assert_eq!(s.rev(), revision, "{id}");
        assert_eq!(s.document_id(), document, "{id}");
        assert!(!s.dirty, "{id}");
    }
    assert!(!s.host_command_allowed("unknown.futureCommand"));
}

#[test]
fn native_host_readonly_keeps_select_copy_find_and_view_working() {
    let mut s = s();
    run(&mut s, "text.insert", json!({"text":"find this native text"}));
    s.set_host_read_only(true);
    let bytes = native(&s);
    let revision = s.rev();
    run(&mut s, "caret.docStart", json!({}));
    run(&mut s, "select.all", json!({}));
    run(&mut s, "edit.copy", json!({}));
    assert!(s.clipboard_text.contains("find this native text"));
    run(&mut s, "edit.find", json!({"text":"native"}));
    run(&mut s, "view.zoom", json!({"value":125}));
    run(&mut s, "review.comments", json!({}));
    assert_eq!(native(&s), bytes);
    assert_eq!(s.rev(), revision);
}

#[test]
fn native_host_readonly_trusted_import_does_not_unlock_interactive_commands() {
    let mut s = s();
    s.set_host_read_only(true);
    let document = s.document_id();
    let imported = wordcraft_doc::Document::from_text("host-owned imported native text");
    let bytes = serde_json::to_vec(&imported).unwrap();
    s.open_from_host(&json!({"path":"native.json","data":cmd::insert::base64_encode(&bytes)})).unwrap();
    assert_ne!(s.document_id(), document);
    assert!(s.host_read_only());
    assert!(text(&s).contains("host-owned imported native text"));
    let imported_document = s.document_id();
    s.add_document(wordcraft_doc::Document::from_text("another trusted document"));
    assert!(s.host_read_only());
    s.switch_document(imported_document).unwrap();
    assert!(s.host_read_only(), "switching open documents must not swap host authority");
    let bytes = native(&s);
    assert!(s.run("text.insert", &json!({"text":"forbidden"})).is_err());
    assert_eq!(native(&s), bytes);
    s.set_host_read_only(false);
    assert!(s.doc.settings.protection.is_none());
    s.doc.settings.protection = Some("readOnly".into());
    assert!(s.run("text.insert", &json!({"text":"still forbidden"})).is_err());
    assert_eq!(s.doc.settings.protection.as_deref(), Some("readOnly"));
}

#[test]
fn native_host_readonly_refuses_a_full_document_import_before_any_state_change() {
    let mut s = s();
    run(&mut s, "text.insert", json!({"text":"retained host content"}));
    let before = native(&s);
    let id = s.document_id();
    let revision = s.rev();
    let imported = crate::io::save_bytes("picked.docx", &wordcraft_doc::Document::from_text("forbidden import")).unwrap();
    s.set_host_read_only(true);
    let error = s.run("file.import", &json!({"path":"picked.docx","data":crate::cmd::insert::base64_encode(&imported)})).unwrap_err().to_string();
    assert!(error.contains("read-only"));
    assert_eq!(s.document_id(), id);
    assert_eq!(s.rev(), revision);
    assert_eq!(native(&s), before);
}
