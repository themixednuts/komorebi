use std::error::Error;
use std::num::NonZeroU16;
use std::num::NonZeroU32;
use std::time::Duration;
use std::time::Instant;

use komorebi_command_store::ActionParameterDocument;
use komorebi_command_store::DurableInvocationLedger;
use komorebi_command_store::LeaseDecision;
use komorebi_command_store::LeaseRequest;
use komorebi_command_store::LedgerTimestamp;
use komorebi_command_store::NamespaceRegistration;
use komorebi_command_store::ReservationDecision;
use komorebi_command_store::ReservationRequest;
use komorebi_protocol::InvocationDigest;
use komorebi_protocol::InvocationId;
use komorebi_protocol::InvocationNamespaceId;
use komorebi_protocol::InvocationSequence;
use komorebi_protocol::PrincipalId;

const SAMPLES: u32 = 1_024;
const P99_BUDGET: Duration = Duration::from_millis(16);

fn percentile(sorted: &[Duration], numerator: usize, denominator: usize) -> Duration {
    let rank = sorted.len().saturating_mul(numerator).div_ceil(denominator);
    sorted[rank.saturating_sub(1)]
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn main() -> Result<(), Box<dyn Error>> {
    let directory = tempfile::tempdir()?;
    let path = directory.path().join("admission-latency.sqlite");
    let principal = PrincipalId::new([1; 32])?;
    let namespace = InvocationNamespaceId::new([2; 16])?;
    let digest = InvocationDigest::new([3; 32])?;
    let count = NonZeroU32::new(SAMPLES).ok_or("sample count must be nonzero")?;
    let mut ledger = DurableInvocationLedger::open(&path)?;

    if ledger.register_namespace(namespace, principal)? != NamespaceRegistration::Registered {
        return Err("fresh benchmark namespace was not registered".into());
    }
    if !matches!(
        ledger.lease(LeaseRequest {
            namespace,
            principal,
            count,
        })?,
        LeaseDecision::Issued(_)
    ) {
        return Err("benchmark sequence lease was not issued".into());
    }

    let mut samples = Vec::with_capacity(SAMPLES as usize);
    for sequence in 1..=u64::from(SAMPLES) {
        let request = ReservationRequest {
            principal,
            invocation_id: InvocationId::new(namespace, InvocationSequence::try_from(sequence)?),
            digest,
            parameters: ActionParameterDocument::new(NonZeroU16::MIN, [9; 64])?,
            reserved_at: LedgerTimestamp::from_unix_millis(1)?,
        };
        let started = Instant::now();
        let decision = ledger.reserve(request)?;
        samples.push(started.elapsed());
        if !matches!(decision, ReservationDecision::Reserved(_)) {
            return Err(format!("sample {sequence} was not reserved: {decision:?}").into());
        }
    }

    samples.sort_unstable();
    let p50 = percentile(&samples, 50, 100);
    let p95 = percentile(&samples, 95, 100);
    let p99 = percentile(&samples, 99, 100);
    let maximum = samples.last().copied().ok_or("no latency samples")?;
    println!(
        "durable reservation samples={SAMPLES} p50_ms={:.3} p95_ms={:.3} p99_ms={:.3} max_ms={:.3} budget_ms={:.3}",
        milliseconds(p50),
        milliseconds(p95),
        milliseconds(p99),
        milliseconds(maximum),
        milliseconds(P99_BUDGET),
    );

    if p99 > P99_BUDGET {
        return Err(format!(
            "durable reservation p99 {:.3} ms exceeds {:.3} ms budget",
            milliseconds(p99),
            milliseconds(P99_BUDGET),
        )
        .into());
    }
    Ok(())
}
