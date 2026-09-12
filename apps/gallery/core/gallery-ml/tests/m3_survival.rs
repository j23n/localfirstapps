//! ADR 0005 R19 survival fixture for M3 (face-pack re-key).
//!
//! A `face_pack_key` change — the thing a SFace+YuNet swap will do — throws
//! away every detection, cluster and merge proposal. The durable record of a
//! naming is the sidecar, not those tables, so a People keyword already on
//! disk must still be there after the wipe, and a stale cluster id the UI is
//! still holding must not silently resolve to that person.
//!
//! SQL + XMP only: no ONNX, no detector, no `FaceEngine::open`. The pack
//! swap itself is still the remaining step; see `apps/gallery/docs/m3-face-pack.md`.

use gallery_meta::{
    read_view, write_faces, Area, FaceDecision, FaceRegionWrite, FaceWriteRequest,
};
use gallery_ml::cache::{ClusterRow, ClusterState, FaceLibraryStats, StoredFace, META_FACE_PACK};
use gallery_ml::{CacheDb, MlError, WorkState};
use gallery_vfs::StdVfs;

/// Stands in for `ModelPack::face_pack_key()` under buffalo_sc.
const OLD_FACE_PACK: &str = "scrfd+w600k#p1a1";
/// Stands in for the same function after the SFace+YuNet hashes land.
const NEW_FACE_PACK: &str = "yunet+sface#p1a1";

const STAMP: &str = "2026-08-03T10:00:00Z";
const ADA_AREA: (f64, f64, f64, f64) = (0.40, 0.35, 0.12, 0.16);

struct Fixture {
    dir: tempfile::TempDir,
    cache: CacheDb,
    photo: String,
    hash: [u8; 32],
    cluster_id: i64,
    sidecar_before: Vec<u8>,
}

impl Fixture {
    fn seed() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let photo_path = dir.path().join("ada.jpg");
        // Bytes are never decoded — this fixture is the cache and the sidecar.
        let photo_bytes = b"m3-survival-photo";
        std::fs::write(&photo_path, photo_bytes).unwrap();
        let photo = photo_path.to_string_lossy().into_owned();
        let hash = gallery_ml::hash::hash_bytes(photo_bytes);

        let cache = CacheDb::open(dir.path().join("gallery-cache.sqlite")).unwrap();
        cache.set_meta(META_FACE_PACK, OLD_FACE_PACK).unwrap();

        let face = StoredFace {
            content_hash: hash,
            face_idx: 0,
            bbox: [80.0, 60.0, 160.0, 180.0],
            landmarks: [
                [90.0, 90.0],
                [140.0, 90.0],
                [115.0, 120.0],
                [95.0, 150.0],
                [135.0, 150.0],
            ],
            score: 0.92,
            quality: 0.71,
            embedding: vec![1.0, 0.0, 0.0],
            image_w: 640,
            image_h: 480,
        };
        cache
            .put_faces(&hash, OLD_FACE_PACK, 640, 480, &[face])
            .unwrap();

        let cluster_id = cache.create_cluster(&[1.0, 0.0, 0.0]).unwrap();
        cache
            .set_cluster_state(cluster_id, ClusterState::Named, Some("Ada"))
            .unwrap();
        cache.set_cluster_member(cluster_id, &hash, 0).unwrap();
        // A second unlabeled cluster + a merge proposal: both must vanish
        // with the named row. The proposal is what in-app review is holding.
        let other = cache.create_cluster(&[0.0, 1.0, 0.0]).unwrap();
        cache.put_merge_proposal(cluster_id, other, 0.81).unwrap();

        // A `done` face-queue row under the old pack. `with_models` also
        // calls `face_mark_stale_for_pack`; reset + set_meta alone would
        // leave this Done.
        cache.face_enqueue(&[photo.clone()]).unwrap();
        assert!(cache.face_begin(&photo).unwrap());
        assert!(cache
            .face_finish_done(&photo, OLD_FACE_PACK, 1, None)
            .unwrap());
        assert_eq!(
            cache.face_item(&photo).unwrap().unwrap().state,
            WorkState::Done
        );

        let area = Area::new(ADA_AREA.0, ADA_AREA.1, ADA_AREA.2, ADA_AREA.3);
        let request = FaceWriteRequest::new(
            [FaceRegionWrite {
                name: "Ada".into(),
                area,
            }],
            OLD_FACE_PACK,
            STAMP,
        )
        .with_image_size(640, 480)
        .with_decisions([FaceDecision::Named {
            area,
            name: "Ada".into(),
        }]);
        write_faces(&StdVfs::new(), &photo, &request).unwrap();

        let sidecar_before = std::fs::read(format!("{photo}.xmp")).unwrap();
        let view = read_view(&sidecar_before).unwrap();
        assert_eq!(view.people_tags(), vec!["People/Ada"]);
        assert_eq!(view.core.face_pack.as_deref(), Some(OLD_FACE_PACK));
        assert_eq!(
            cache
                .cluster(cluster_id)
                .unwrap()
                .unwrap()
                .person_name
                .as_deref(),
            Some("Ada")
        );

        Fixture {
            dir,
            cache,
            photo,
            hash,
            cluster_id,
            sidecar_before,
        }
    }

    fn sidecar_path(&self) -> String {
        self.dir
            .path()
            .join("ada.jpg.xmp")
            .to_string_lossy()
            .into_owned()
    }

    fn sidecar_bytes(&self) -> Vec<u8> {
        std::fs::read(self.sidecar_path()).unwrap()
    }
}

/// The first thing [`gallery_ml::FaceEngine::name_cluster`] does with an id:
/// look it up, or refuse. A missing row is [`MlError::ClusterNotFound`], not
/// a guess from the sidecar.
fn resolve_cluster_for_naming(cache: &CacheDb, cluster_id: i64) -> Result<ClusterRow, MlError> {
    cache
        .cluster(cluster_id)?
        .ok_or(MlError::ClusterNotFound { id: cluster_id })
}

/// The `face_pack_key` check inside [`gallery_ml::FaceEngine::with_models`]:
/// reset derived face tables, record the new key, then stale `face_work`
/// rows produced under a different pack.
fn adopt_face_pack_key(cache: &CacheDb, new_key: &str) {
    if cache.meta(META_FACE_PACK).unwrap().as_deref() != Some(new_key) {
        cache.reset_face_results().unwrap();
        cache.set_meta(META_FACE_PACK, new_key).unwrap();
    }
    cache.face_mark_stale_for_pack(new_key).unwrap();
}

/// A pack re-key empties derived face tables, leaves `People/Ada` on disk,
/// and turns the old cluster id into [`MlError::ClusterNotFound`].
#[test]
fn a_pack_rekey_empties_clusters_and_leaves_xmp_people_on_disk() {
    let f = Fixture::seed();
    let old_id = f.cluster_id;

    let before = resolve_cluster_for_naming(&f.cache, old_id).unwrap();
    assert_eq!(before.person_name.as_deref(), Some("Ada"));

    // The SFace+YuNet swap is a new `face_pack_key`. FaceEngine::with_models
    // compares it to `meta.face_pack` and, on mismatch, does the same three
    // steps this helper does.
    adopt_face_pack_key(&f.cache, NEW_FACE_PACK);

    assert_eq!(
        f.cache.meta(META_FACE_PACK).unwrap().as_deref(),
        Some(NEW_FACE_PACK)
    );
    assert_eq!(
        f.cache.face_item(&f.photo).unwrap().unwrap().state,
        WorkState::Stale,
        "a Done face_work row under the old pack must become Stale"
    );
    assert_eq!(
        f.cache.face_library_stats().unwrap(),
        FaceLibraryStats::default(),
        "faces, clusters and merge proposals must all be gone"
    );
    assert!(f.cache.clusters().unwrap().is_empty());
    assert!(f.cache.cluster_members(old_id).unwrap().is_empty());
    assert!(f.cache.faces_for_hash(&f.hash).unwrap().is_empty());
    assert!(f.cache.named_faces_for_hash(&f.hash).unwrap().is_empty());
    assert_eq!(f.cache.face_scan(&f.hash, OLD_FACE_PACK).unwrap(), None);
    assert!(f.cache.merge_proposals().unwrap().is_empty());

    let sidecar_after = f.sidecar_bytes();
    assert_eq!(
        sidecar_after, f.sidecar_before,
        "reset_face_results rewrote the sidecar"
    );
    let view = read_view(&sidecar_after).unwrap();
    assert_eq!(view.people_tags(), vec!["People/Ada"]);
    assert_eq!(view.person_in_image, vec!["Ada"]);
    assert_eq!(view.regions.len(), 1);
    assert_eq!(view.regions[0].name.as_deref(), Some("Ada"));
    assert_eq!(view.core.people, vec!["People/Ada"]);
    assert_eq!(view.core.face_pack.as_deref(), Some(OLD_FACE_PACK));

    let err = resolve_cluster_for_naming(&f.cache, old_id).unwrap_err();
    assert!(
        matches!(err, MlError::ClusterNotFound { id } if id == old_id),
        "stale id {old_id} must not resolve to Ada: {err:?}"
    );

    // The AUTOINCREMENT high-water mark is left in place on purpose: a reused
    // id would let a review screen name a different group of strangers.
    let next = f.cache.create_cluster(&[0.0, 0.0, 1.0]).unwrap();
    assert!(next > old_id, "id {next} was reused after {old_id}");
    f.cache
        .set_cluster_state(next, ClusterState::Named, Some("Zoe"))
        .unwrap();
    assert!(f.cache.cluster(old_id).unwrap().is_none());
    assert!(
        matches!(
            resolve_cluster_for_naming(&f.cache, old_id),
            Err(MlError::ClusterNotFound { id }) if id == old_id
        ),
        "a new named cluster must not resurrect {old_id}"
    );
    assert_eq!(
        f.cache
            .cluster(next)
            .unwrap()
            .unwrap()
            .person_name
            .as_deref(),
        Some("Zoe")
    );
    assert_eq!(
        read_view(&f.sidecar_bytes()).unwrap().people_tags(),
        vec!["People/Ada"],
        "a post-swap cache name leaked onto disk"
    );
}
