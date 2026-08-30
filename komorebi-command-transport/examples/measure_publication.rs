use std::sync::Arc;
use std::time::Instant;

use komorebi_command_transport::EventSubscriptions;
use komorebi_command_transport::SubscriberClass;
use komorebi_command_transport::SubscriptionStart;
use komorebi_protocol::FrameCost;
use komorebi_protocol::ManagerEpoch;
use komorebi_protocol::TopicFilter;
use komorebi_protocol::TopicId;

const STALLED_READERS: usize = 32;
const SAMPLES: usize = 1024;
const EVENT_PAYLOAD_BYTES: usize = 64;
const P99_BUDGET_NS: u128 = 100_000;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let epoch = ManagerEpoch::new([1; 16])?;
    let topic = TopicId::try_from(1)?;
    let cost = FrameCost::for_payload(EVENT_PAYLOAD_BYTES)?;
    let mut subscriptions = EventSubscriptions::new(epoch, Arc::new(0_u64));
    let _readers = stalled_readers(&mut subscriptions)?;
    let mut samples = Vec::with_capacity(SAMPLES);

    for revision in 1..=u64::try_from(SAMPLES)? {
        let started = Instant::now();
        subscriptions.publish(Arc::new(revision), topic, revision, cost)?;
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();

    let p99 = percentile(&samples, 99);
    println!(
        "readers={STALLED_READERS} samples={SAMPLES} p50_ns={} p95_ns={} p99_ns={p99} max_ns={} budget_ns={P99_BUDGET_NS}",
        percentile(&samples, 50),
        percentile(&samples, 95),
        samples.last().copied().unwrap_or_default(),
    );
    if p99 > P99_BUDGET_NS {
        return Err(format!("publication p99 {p99} ns exceeded {P99_BUDGET_NS} ns").into());
    }
    Ok(())
}

fn stalled_readers(
    subscriptions: &mut EventSubscriptions<u64, u64>,
) -> Result<Vec<SubscriptionStart<u64, u64>>, Box<dyn std::error::Error>> {
    (0..STALLED_READERS)
        .map(|_| subscriptions.subscribe(TopicFilter::All, SubscriberClass::FirstParty))
        .collect::<Result<_, _>>()
        .map_err(Into::into)
}

fn percentile(samples: &[u128], percent: usize) -> u128 {
    let rank = samples
        .len()
        .saturating_mul(percent)
        .div_ceil(100)
        .saturating_sub(1);
    samples.get(rank).copied().unwrap_or_default()
}
