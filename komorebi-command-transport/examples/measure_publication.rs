use std::time::Instant;

use komorebi_command_transport::LanePublisher;
use komorebi_command_transport::LaneReceiver;
use komorebi_command_transport::bounded_lane;
use komorebi_protocol::FrameCost;
use komorebi_protocol::LaneLimits;

const STALLED_READERS: usize = 32;
const SAMPLES: usize = 1024;
const EVENT_PAYLOAD_BYTES: usize = 64;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (mut publishers, _receivers) = stalled_readers()?;
    let cost = FrameCost::for_payload(EVENT_PAYLOAD_BYTES)?;
    let mut samples = Vec::with_capacity(SAMPLES);

    for _ in 0..SAMPLES {
        let started = Instant::now();
        for publisher in &mut publishers {
            publisher.try_publish(cost, ())?;
        }
        samples.push(started.elapsed().as_nanos());
    }
    samples.sort_unstable();

    println!(
        "readers={STALLED_READERS} samples={SAMPLES} p50_ns={} p95_ns={} p99_ns={} max_ns={}",
        percentile(&samples, 50),
        percentile(&samples, 95),
        percentile(&samples, 99),
        samples.last().copied().unwrap_or_default(),
    );
    Ok(())
}

type StalledReaders = (Vec<LanePublisher<()>>, Vec<LaneReceiver<()>>);

fn stalled_readers() -> Result<StalledReaders, Box<dyn std::error::Error>> {
    let mut publishers = Vec::with_capacity(STALLED_READERS);
    let mut receivers = Vec::with_capacity(STALLED_READERS);
    for _ in 0..STALLED_READERS {
        let (publisher, receiver) = bounded_lane(LaneLimits::FIRST_PARTY_DATA)?;
        publishers.push(publisher);
        receivers.push(receiver);
    }
    Ok((publishers, receivers))
}

fn percentile(samples: &[u128], percent: usize) -> u128 {
    let rank = samples
        .len()
        .saturating_mul(percent)
        .div_ceil(100)
        .saturating_sub(1);
    samples.get(rank).copied().unwrap_or_default()
}
