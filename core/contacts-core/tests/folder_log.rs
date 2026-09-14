use contacts_core::{
    append_deleted, append_saved, read_ops, write, Card, TYPE_CONTACT_DELETED, TYPE_CONTACT_SAVED,
};
use localcore_vfs::{MemVfs, Vfs};

#[test]
fn folder_log_records_operations() {
    let vfs = MemVfs::new();
    append_saved(&vfs, "/lib", "phone", "a1").unwrap();
    append_deleted(&vfs, "/lib", "phone", "a1").unwrap();
    let evs = read_ops(&vfs, "/lib").unwrap();
    assert_eq!(evs.len(), 2);
    assert_eq!(evs[0].event_type, TYPE_CONTACT_SAVED);
    assert_eq!(evs[0].body["id"], "a1");
    assert_eq!(evs[1].event_type, TYPE_CONTACT_DELETED);
}

#[test]
fn folder_log_does_not_touch_vcards() {
    let vfs = MemVfs::new();
    let mut c = Card::new("alice.vcf");
    c.local_id = "a1".into();
    c.full_name = "Alice".into();
    vfs.insert("/lib/alice.vcf", write(&c).into_bytes());
    append_saved(&vfs, "/lib", "phone", "a1").unwrap();
    assert_eq!(vfs.read("/lib/alice.vcf").unwrap(), write(&c).into_bytes());
}
