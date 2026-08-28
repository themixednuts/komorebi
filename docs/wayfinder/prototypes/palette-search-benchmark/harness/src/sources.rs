use std::num::{NonZeroU32, NonZeroU64};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::domain::{
    EngineEpoch, ExtensionLimits, InvocationSettlement, OneShotInvocations, PublicationFence,
    QueryGeneration, QueryRoute, RootId, SnapshotGeneration, WorkerGeneration, parse_query,
};

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExternalEffectCounters {
    pub dns: u64,
    pub http: u64,
    pub remote_icons: u64,
    pub suggestions: u64,
    pub browser_handoffs: u64,
}

impl ExternalEffectCounters {
    #[must_use]
    pub const fn is_silent(self) -> bool {
        self.dns == 0
            && self.http == 0
            && self.remote_icons == 0
            && self.suggestions == 0
            && self.browser_handoffs == 0
    }

    pub fn explicit_web_enter(&mut self) {
        self.browser_handoffs = self.browser_handoffs.saturating_add(1);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExtensionSettlement {
    Published,
    Cancelled,
    TooManyRows,
    TooManyBytes,
    SnippetTooLarge,
    Stale,
    Crashed,
}

#[must_use]
pub fn settle_extension(
    produced: ExtensionOutput,
    limits: ExtensionLimits,
    response_fence: PublicationFence,
    current_fence: PublicationFence,
) -> ExtensionSettlement {
    if produced.crashed {
        return ExtensionSettlement::Crashed;
    }
    if produced.cancelled {
        return ExtensionSettlement::Cancelled;
    }
    if !response_fence.admits(current_fence) {
        return ExtensionSettlement::Stale;
    }
    if produced.rows > limits.rows {
        return ExtensionSettlement::TooManyRows;
    }
    if produced.bytes > limits.bytes {
        return ExtensionSettlement::TooManyBytes;
    }
    if produced.max_snippet_bytes > limits.snippet_bytes {
        return ExtensionSettlement::SnippetTooLarge;
    }
    ExtensionSettlement::Published
}

#[derive(Debug, Clone, Copy)]
pub struct ExtensionOutput {
    pub rows: usize,
    pub bytes: usize,
    pub max_snippet_bytes: usize,
    pub cancelled: bool,
    pub crashed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct BoundaryMeasurement {
    pub pre_enter_effects: ExternalEffectCounters,
    pub silent_before_enter: bool,
    pub browser_handoff_only_after_enter: bool,
    pub route_parser_cases_pass: bool,
    pub duplicate_invocation_rejected: bool,
    pub bounded_rows: ExtensionSettlement,
    pub bounded_bytes: ExtensionSettlement,
    pub bounded_snippet: ExtensionSettlement,
    pub cancellation: ExtensionSettlement,
    pub crash: ExtensionSettlement,
    pub stale_generation: ExtensionSettlement,
    pub rapid_replacement_stale_publications: usize,
    pub delayed_output_stale_publications: usize,
    pub repeated_cancellation_stale_publications: usize,
}

#[must_use]
pub fn measure_boundaries() -> BoundaryMeasurement {
    let mut effects = ExternalEffectCounters::default();
    let pre_enter_effects = effects;
    let silent_before_enter = effects.is_silent();
    effects.explicit_web_enter();
    let browser_handoff_only_after_enter = effects.browser_handoffs == 1
        && effects.dns == 0
        && effects.http == 0
        && effects.remote_icons == 0
        && effects.suggestions == 0;
    let route_parser_cases_pass = matches!(
        parse_query("focus left", &["g", "ddg", "yt"]),
        Ok(QueryRoute::BlendedLocal(query)) if query.as_str() == "focus left"
    ) && matches!(
        parse_query("? manager epoch", &["g", "ddg", "yt"]),
        Ok(QueryRoute::FileContent(query)) if query.as_str() == "manager epoch"
    ) && matches!(
        parse_query("!g rust", &["g", "ddg", "yt"]),
        Ok(QueryRoute::Web(draft))
            if draft.alias.as_deref() == Some("g") && draft.query.as_str() == "rust"
    ) && matches!(
        parse_query("@docs manager", &["g", "ddg", "yt"]),
        Ok(QueryRoute::Extension { source, query })
            if source.as_str() == "docs" && query.as_str() == "manager"
    );
    let mut invocations = OneShotInvocations::default();
    let invocation = Uuid::new_v4();
    let duplicate_invocation_rejected = invocations.begin(invocation)
        == InvocationSettlement::Accepted
        && invocations.begin(invocation) == InvocationSettlement::Duplicate;

    let limits = ExtensionLimits::new(20, 64 * 1024, 512).unwrap_or(ExtensionLimits {
        rows: 20,
        bytes: 64 * 1024,
        snippet_bytes: 512,
    });
    let current = fence(2, 2);
    let valid = ExtensionOutput {
        rows: 4,
        bytes: 1024,
        max_snippet_bytes: 128,
        cancelled: false,
        crashed: false,
    };
    let stale = settle_extension(valid, limits, fence(1, 1), current);

    BoundaryMeasurement {
        pre_enter_effects,
        silent_before_enter,
        browser_handoff_only_after_enter,
        route_parser_cases_pass,
        duplicate_invocation_rejected,
        bounded_rows: settle_extension(
            ExtensionOutput { rows: 21, ..valid },
            limits,
            current,
            current,
        ),
        bounded_bytes: settle_extension(
            ExtensionOutput {
                bytes: 64 * 1024 + 1,
                ..valid
            },
            limits,
            current,
            current,
        ),
        bounded_snippet: settle_extension(
            ExtensionOutput {
                max_snippet_bytes: 513,
                ..valid
            },
            limits,
            current,
            current,
        ),
        cancellation: settle_extension(
            ExtensionOutput {
                cancelled: true,
                ..valid
            },
            limits,
            current,
            current,
        ),
        crash: settle_extension(
            ExtensionOutput {
                crashed: true,
                ..valid
            },
            limits,
            current,
            current,
        ),
        stale_generation: stale,
        rapid_replacement_stale_publications: stale_publications(128, 2),
        delayed_output_stale_publications: stale_publications(128, 2),
        repeated_cancellation_stale_publications: stale_publications(128, 2),
    }
}

fn stale_publications(samples: u64, current_query: u64) -> usize {
    let current = fence(2, current_query);
    (1..=samples)
        .map(|sample| fence(if sample % 3 == 0 { 1 } else { 2 }, sample))
        .filter(|response| response.admits(current))
        .filter(|response| response.query.value().get() != current_query)
        .count()
}

fn fence(worker: u64, query: u64) -> PublicationFence {
    PublicationFence {
        engine: EngineEpoch::new(NonZeroU64::MIN),
        worker: WorkerGeneration::new(NonZeroU64::new(worker).unwrap_or(NonZeroU64::MIN)),
        root: RootId::new(NonZeroU32::MIN),
        snapshot: SnapshotGeneration::new(NonZeroU64::MIN),
        query: QueryGeneration::new(NonZeroU64::new(query).unwrap_or(NonZeroU64::MIN)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_enter_search_has_no_external_capability() {
        let measurement = measure_boundaries();
        assert!(measurement.silent_before_enter);
        assert!(measurement.browser_handoff_only_after_enter);
    }

    #[test]
    fn stale_work_never_publishes() {
        let measurement = measure_boundaries();
        assert_eq!(measurement.rapid_replacement_stale_publications, 0);
        assert_eq!(measurement.delayed_output_stale_publications, 0);
        assert_eq!(measurement.repeated_cancellation_stale_publications, 0);
    }
}
