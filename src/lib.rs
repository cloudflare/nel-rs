#![recursion_limit = "512"]

mod error;
mod report;

#[macro_use]
extern crate lazy_static;

use deadqueue::limited::Queue;
use futures::future::{Fuse, FutureExt};
use futures::{pin_mut, select, Future};
use rand::{random, seq::SliceRandom, thread_rng};
use report::FailedReport;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use ttl_cache::TtlCache;
use url::Url;

pub use error::Error;
pub use report::NELReport;

const RETRY_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
struct NELPolicy {
    report_to: String,
    success_fraction: f32,
    failure_fraction: f32,
}

lazy_static! {
    static ref NEL_POLICY_CACHE: Mutex<TtlCache<String, NELPolicy>> = Mutex::new(TtlCache::new(50));
    static ref GROUP_POLICY_CACHE: Mutex<TtlCache<String, Vec<String>>> =
        Mutex::new(TtlCache::new(50));
    static ref REPORT_QUEUE: Queue<NELReport> = Queue::new(256);
}

#[derive(Serialize, Deserialize)]
struct NelHeader {
    /// Name of group to send reports to.
    report_to: String,
    /// Lifetime of policy in seconds.
    max_age: u64,
    #[serde(default)]
    include_subdomains: bool,
    #[serde(default)]
    success_fraction: f32,
    #[serde(default = "default_failure_fraction")]
    failure_fraction: f32,
}

const fn default_failure_fraction() -> f32 {
    1.0
}

/// nel_header takes the value of a NEL header and caches the specified policy.
pub fn nel_header(host: &str, hdr: &str) {
    let parsed = match serde_json::from_str::<NelHeader>(hdr) {
        Ok(parsed) => parsed,
        Err(_) => return,
    };

    let valid = !parsed.report_to.is_empty()
        && (0.0..=1.0).contains(&parsed.success_fraction)
        && (0.0..=1.0).contains(&parsed.failure_fraction);
    if !valid {
        return;
    }

    if let Ok(mut guard) = NEL_POLICY_CACHE.lock() {
        if parsed.max_age == 0 {
            guard.remove(host);
        } else {
            let policy = NELPolicy {
                report_to: parsed.report_to,
                success_fraction: parsed.success_fraction,
                failure_fraction: parsed.failure_fraction,
            };
            guard.insert(
                host.to_string(),
                policy,
                Duration::from_secs(parsed.max_age),
            );
        }
    }
}

#[derive(Serialize, Deserialize)]
struct ReportToHeader {
    /// Name of this group of endpoints.
    group: String,
    /// Lifetime of policy in seconds.
    max_age: u64,
    endpoints: Vec<ReportEndpoint>,
}

#[derive(Serialize, Deserialize)]
struct ReportEndpoint {
    url: String,
}

/// report_to_header takes the value of the Report-To header and saves any group endpoint URLs.
pub fn report_to_header(host: &str, hdr: &str) {
    let parsed = match serde_json::from_str::<ReportToHeader>(hdr) {
        Ok(parsed) => parsed,
        Err(_) => return,
    };

    let valid = !parsed.group.is_empty()
        && !parsed.endpoints.is_empty()
        && parsed
            .endpoints
            .iter()
            .fold(true, |ok, ep| ok && !ep.url.is_empty());
    if !valid {
        return;
    }

    let key = format!("{}:{}", host, parsed.group);

    if let Ok(mut guard) = GROUP_POLICY_CACHE.lock() {
        if parsed.max_age == 0 {
            guard.remove(&key);
        } else {
            let endpoints = parsed.endpoints.iter().map(|ep| ep.url.clone()).collect();
            guard.insert(key, endpoints, Duration::from_secs(parsed.max_age));
        }
    }
}

/// submit_report adds a report to the queue to be sent to the server.
pub fn submit_report(report: NELReport) {
    let _ = REPORT_QUEUE.try_push(report);
}

/// handle_reports receives NEL reports and submits them to the reporting endpoint.
///
/// As input, it takes:
///   - an async method for sleeping, and
///   - an async method that takes a URI and POST body as input, sends a POST request, and returns
///     a boolean indicating if the request succeeded or not.
pub async fn handle_reports<F, G, FFut, GFut>(sleep: F, post: G)
where
    F: Fn(Duration) -> FFut,
    G: Fn(String, String) -> GFut,
    FFut: Future<Output = ()>,
    GFut: Future<Output = bool>,
{
    let pop = REPORT_QUEUE.pop().fuse();

    let failed_queue: Queue<FailedReport> = Queue::new(256);
    let fail_timeout = Fuse::terminated();
    let mut next_failed: Option<FailedReport> = None;

    pin_mut!(pop, fail_timeout);

    // TODO: Submit many reports to the same group at once.
    loop {
        select! {
            report = pop => {
                // Submit report.
                let payload = report.serialize();
                let success = match choose_endpoint(&report, true) {
                    Some(endpoint) => post(endpoint, payload).await,
                    None => true, // No cached endpoint to submit report to.
                };

                // If submitting the report failed, save it and try again later.
                if !success {
                    let failed = FailedReport{
                        last_try: Instant::now(),
                        original: report,
                    };
                    if next_failed.is_none() {
                        fail_timeout.set(sleep(RETRY_TIMEOUT).fuse());
                        next_failed = Some(failed);
                    } else {
                        let _ = failed_queue.try_push(failed);
                    }
                }

                // Start waiting for the next report.
                pop.set(REPORT_QUEUE.pop().fuse());
            },
            _ = fail_timeout => {
                // Submit next_failed report.
                let report = &next_failed.as_ref().unwrap().original;
                let payload = report.serialize();
                let success = match choose_endpoint(report, false) {
                    Some(endpoint) => post(endpoint, payload).await,
                    None => true, // No cached endpoint to submit report to.
                };

                // If submitting the report failed, save it and try again later.
                if !success {
                    let _ = failed_queue.try_push(FailedReport{
                        last_try: Instant::now(),
                        original: next_failed.unwrap().original,
                    });
                }

                // Pop the next failed report and prepare a timer.
                if let Some(failed) = failed_queue.try_pop() {
                    let dur = RETRY_TIMEOUT
                        .checked_sub(Instant::now().duration_since(failed.last_try))
                        .unwrap_or_else(|| Duration::from_millis(10));
                    fail_timeout.set(sleep(dur).fuse());
                    next_failed = Some(failed)
                } else {
                    fail_timeout.set(Fuse::terminated());
                    next_failed = None;
                }
            },
        }
    }
}

fn choose_endpoint(report: &NELReport, evaluate_drop: bool) -> Option<String> {
    // Pull up the policies that correspond to this report.
    let host = match &report.host_override {
        Some(host) => host.clone(),
        None => {
            let report_url = Url::parse(&report.url).ok()?;
            report_url.host_str()?.to_owned()
        }
    };
    let nel_policy = {
        let guard = NEL_POLICY_CACHE.lock().ok()?;
        let policy = guard.get(&host)?;
        policy.clone()
    };
    let group_policy = {
        let group_policy_key = format!("{}:{}", host, &nel_policy.report_to);
        let guard = GROUP_POLICY_CACHE.lock().ok()?;
        let policy = guard.get(&group_policy_key)?;
        policy.clone()
    };

    // Decide if report should be dropped.
    if evaluate_drop {
        if report.is_success() {
            if random::<f32>() >= nel_policy.success_fraction {
                return None;
            }
        } else {
            if random::<f32>() >= nel_policy.failure_fraction {
                return None;
            }
        }
    }

    // Return random endpoint if not dropped.
    Some(group_policy.choose(&mut thread_rng())?.clone())
}
