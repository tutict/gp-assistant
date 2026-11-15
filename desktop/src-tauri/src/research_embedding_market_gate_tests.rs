use crate::research::background::ResearchEmbeddingJobGates;

#[test]
fn embedding_jobs_coalesce_per_market_without_losing_the_other_market_request() {
    let gates = ResearchEmbeddingJobGates::new();
    let cn = gates.for_market("cn");
    let hk = gates.for_market("hk");

    assert!(cn.request());
    assert!(hk.request());
    assert!(!cn.request());
    assert!(!hk.request());
    cn.begin_cycle();
    hk.begin_cycle();
    assert!(!cn.finish_cycle());
    assert!(!hk.finish_cycle());
}
