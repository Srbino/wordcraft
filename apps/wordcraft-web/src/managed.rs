//! Bounded native snapshot acknowledgements for opt-in host-managed saves.

use std::collections::VecDeque;

const MAX_AUTOMATIC_ALIASES: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotKey {
    pub request_id: String,
    pub document: String,
    pub revision: String,
}

struct ManualSave {
    request_id: String,
    document: String,
    ticket: u64,
    snapshot: Option<SnapshotKey>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Completion {
    pub ticket: Option<u64>,
    pub mark_saved: bool,
}

/// Keep a manual save independent of a bounded cohort of identical automatic snapshots.
/// Any known cohort alias can settle that exact revision; newer edits still remain dirty.
#[derive(Default)]
pub struct ManagedSaves {
    manual: Option<ManualSave>,
    automatic: VecDeque<SnapshotKey>,
}

impl ManagedSaves {
    pub fn pending(&self) -> bool {
        self.manual.is_some()
    }

    pub fn start(&mut self, document: &str, ticket: u64) -> Result<String, String> {
        if self.pending() {
            return Err("native save already pending".into());
        }
        let request_id = format!("native-save-{ticket}");
        self.manual = Some(ManualSave { request_id: request_id.clone(), document: document.into(), ticket, snapshot: None });
        Ok(request_id)
    }

    pub fn check_export(&self, snapshot: &SnapshotKey) -> Result<(), String> {
        if let Some(manual) = &self.manual
            && manual.request_id == snapshot.request_id
            && (manual.document != snapshot.document || manual.snapshot.as_ref().is_some_and(|old| old != snapshot))
        {
            return Err("manual save snapshot already bound to another document or revision".into());
        }
        Ok(())
    }

    /// Call only after the native snapshot was validated and successfully posted to the host.
    pub fn exported(&mut self, snapshot: SnapshotKey) -> Result<(), String> {
        self.check_export(&snapshot)?;
        if let Some(manual) = &mut self.manual
            && manual.request_id == snapshot.request_id
        {
            manual.snapshot = Some(snapshot);
        } else {
            if self.automatic.back().is_none_or(|old| old.document != snapshot.document || old.revision != snapshot.revision) {
                self.automatic.clear();
            }
            if !self.automatic.contains(&snapshot) {
                if self.automatic.len() >= MAX_AUTOMATIC_ALIASES {
                    self.automatic.pop_front();
                }
                self.automatic.push_back(snapshot);
            }
        }
        Ok(())
    }

    pub fn knows(&self, snapshot: &SnapshotKey) -> bool {
        self.manual.as_ref().and_then(|m| m.snapshot.as_ref()) == Some(snapshot) || self.automatic.contains(snapshot)
    }

    pub fn knows_failure(&self, request_id: &str) -> bool {
        self.manual.as_ref().is_some_and(|m| m.request_id == request_id)
    }

    pub fn acknowledge(&mut self, snapshot: &SnapshotKey, document: &str, revision: &str) -> Option<Completion> {
        let ticket = if self.manual.as_ref().and_then(|m| m.snapshot.as_ref()) == Some(snapshot) {
            self.manual.take().map(|m| m.ticket)
        } else if self.automatic.contains(snapshot) {
            self.automatic.clear();
            None
        } else {
            return None;
        };
        Some(Completion { ticket, mark_saved: snapshot.document == document && snapshot.revision == revision })
    }

    pub fn fail(&mut self, request_id: &str) -> Option<u64> {
        if self.knows_failure(request_id) { self.manual.take().map(|m| m.ticket) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(id: &str, revision: &str) -> SnapshotKey {
        SnapshotKey { request_id: id.into(), document: "document".into(), revision: revision.into() }
    }
    #[test]
    fn stale_manual_ack_settles_its_ticket_after_a_newer_autosnapshot() {
        let mut saves = ManagedSaves::default();
        let id = saves.start("document", 7).unwrap();
        let manual = key(&id, "r1");
        saves.exported(manual.clone()).unwrap();
        saves.exported(key("auto", "r2")).unwrap();
        assert_eq!(saves.acknowledge(&manual, "document", "r2"), Some(Completion { ticket: Some(7), mark_saved: false }));
        assert!(saves.start("document", 8).is_ok());
    }
    #[test]
    fn only_an_exported_exact_document_and_revision_can_mark_saved() {
        let mut saves = ManagedSaves::default();
        let id = saves.start("document", 7).unwrap();
        let manual = key(&id, "r1");
        assert!(!saves.knows(&manual));
        assert_eq!(saves.acknowledge(&manual, "document", "r1"), None);
        saves.exported(manual.clone()).unwrap();
        assert!(saves.knows(&manual));
        let mut wrong = manual.clone();
        wrong.document = "other".into();
        assert_eq!(saves.acknowledge(&wrong, "document", "r1"), None);
        assert_eq!(saves.acknowledge(&key("unknown", "r1"), "document", "r1"), None);
        assert!(saves.start("document", 8).is_err());
        assert_eq!(saves.acknowledge(&manual, "document", "r1"), Some(Completion { ticket: Some(7), mark_saved: true }));
        assert_eq!(saves.acknowledge(&manual, "document", "r1"), None);
    }
    #[test]
    fn changed_draft_or_document_keeps_dirty_but_completes_the_known_ticket() {
        for (document, revision) in [("document", "r1:draft2"), ("replacement", "r1")] {
            let mut saves = ManagedSaves::default();
            let id = saves.start("document", 4).unwrap();
            let snapshot = key(&id, "r1");
            saves.exported(snapshot.clone()).unwrap();
            assert_eq!(saves.acknowledge(&snapshot, document, revision), Some(Completion { ticket: Some(4), mark_saved: false }));
        }
    }
    #[test]
    fn manual_snapshot_cannot_be_overwritten_and_unknown_failure_cannot_consume_it() {
        let mut saves = ManagedSaves::default();
        let id = saves.start("document", 4).unwrap();
        saves.exported(key(&id, "r1")).unwrap();
        assert!(saves.exported(key(&id, "r2")).is_err());
        let mut wrong = key(&id, "r1");
        wrong.document = "replacement".into();
        assert!(saves.exported(wrong).is_err());
        assert_eq!(saves.fail("unknown"), None);
        assert_eq!(saves.fail(&id), Some(4));
        assert_eq!(saves.fail(&id), None);
        assert!(saves.start("document", 5).is_ok());
    }
    #[test]
    fn autosnapshot_ack_has_no_manual_ticket_and_requires_exact_current_token() {
        let mut saves = ManagedSaves::default();
        let snapshot = key("auto", "r1");
        saves.exported(snapshot.clone()).unwrap();
        assert_eq!(saves.acknowledge(&snapshot, "document", "r2"), Some(Completion { ticket: None, mark_saved: false }));
        saves.exported(snapshot.clone()).unwrap();
        assert_eq!(saves.acknowledge(&snapshot, "document", "r1"), Some(Completion { ticket: None, mark_saved: true }));
    }
    #[test]
    fn identical_automatic_exports_keep_both_known_aliases_until_first_ack() {
        let mut saves = ManagedSaves::default();
        let a = key("auto-a", "r1");
        let b = key("auto-b", "r1");
        saves.exported(a.clone()).unwrap();
        saves.exported(b.clone()).unwrap();
        assert!(saves.knows(&a));
        assert!(saves.knows(&b));
        assert_eq!(saves.acknowledge(&key("unknown", "r1"), "document", "r1"), None);
        assert_eq!(saves.acknowledge(&a, "document", "r1"), Some(Completion { ticket: None, mark_saved: true }));
        assert_eq!(saves.acknowledge(&b, "document", "r1"), None);
    }
    #[test]
    fn identical_automatic_alias_ack_cannot_clean_newer_edit_draft_or_document() {
        for (document, revision) in [("document", "r2"), ("document", "r1:draft2"), ("other", "r1")] {
            let mut saves = ManagedSaves::default();
            let a = key("auto-a", "r1");
            saves.exported(a.clone()).unwrap();
            saves.exported(key("auto-b", "r1")).unwrap();
            assert_eq!(saves.acknowledge(&a, document, revision), Some(Completion { ticket: None, mark_saved: false }));
        }
    }
    #[test]
    fn automatic_alias_cohort_is_bounded_and_changes_with_document_or_revision() {
        let mut saves = ManagedSaves::default();
        for i in 0..17 {
            saves.exported(key(&format!("auto-{i}"), "r1")).unwrap();
        }
        assert!(!saves.knows(&key("auto-0", "r1")));
        assert!(saves.knows(&key("auto-1", "r1")));
        assert!(saves.knows(&key("auto-16", "r1")));
        let fresh = key("fresh", "r2");
        saves.exported(fresh.clone()).unwrap();
        assert!(!saves.knows(&key("auto-16", "r1")));
        assert!(saves.knows(&fresh));
        let mut other = key("other-doc", "r2");
        other.document = "replacement".into();
        saves.exported(other.clone()).unwrap();
        assert!(!saves.knows(&fresh));
        assert!(saves.knows(&other));
    }
    #[test]
    fn automatic_alias_ack_does_not_consume_a_manual_ticket() {
        let mut saves = ManagedSaves::default();
        let id = saves.start("document", 7).unwrap();
        let manual = key(&id, "r1");
        saves.exported(manual.clone()).unwrap();
        let a = key("auto-a", "r1");
        saves.exported(a.clone()).unwrap();
        saves.exported(key("auto-b", "r1")).unwrap();
        assert_eq!(saves.acknowledge(&a, "document", "r1"), Some(Completion { ticket: None, mark_saved: true }));
        assert!(saves.pending());
        assert_eq!(saves.acknowledge(&manual, "document", "r1"), Some(Completion { ticket: Some(7), mark_saved: true }));
        assert!(!saves.pending());
    }
}
