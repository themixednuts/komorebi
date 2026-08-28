use super::{PutRejection, StorageKey, StorageRevision, StoreLimits, StoreState};

fn limits() -> StoreLimits {
    StoreLimits {
        maximum_key: 16,
        maximum_value: 4,
        maximum_entries: 2,
        quota: 6,
    }
}

#[test]
fn put_plan_enforces_cas_and_quota_without_effects() {
    let key = StorageKey::parse("one", 16).expect("valid key");
    let plan = StoreState::default()
        .plan_put(key.clone(), None, vec![1; 4], limits())
        .expect("initial put");
    assert_eq!(plan.revision, StorageRevision::INITIAL);
    assert!(matches!(
        plan.next.plan_put(key.clone(), None, vec![2], limits()),
        Err(PutRejection::Conflict { .. })
    ));
    let other = StorageKey::parse("two", 16).expect("valid key");
    assert_eq!(
        plan.next
            .plan_put(other.clone(), None, vec![2; 3], limits())
            .err(),
        Some(PutRejection::Quota)
    );
    let second = plan
        .next
        .plan_put(other, None, vec![2; 2], limits())
        .expect("second entry fits both limits");
    let third = StorageKey::parse("three", 16).expect("valid key");
    assert_eq!(
        second
            .next
            .plan_put(third, None, Vec::new(), limits())
            .err(),
        Some(PutRejection::EntryLimit)
    );
}

#[test]
fn logical_identifiers_cannot_be_paths() {
    for rejected in ["", ".", "..", "../other", "other\\file", "C:"] {
        assert!(StorageKey::parse(rejected, 16).is_err());
    }
}
