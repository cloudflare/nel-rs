#![recursion_limit = "512"]

mod error;
pub mod report;

#[macro_use]
extern crate lazy_static;

use deadqueue::limited::Queue;
use futures::future::{Fuse, FutureExt};
use futures::{pin_mut, select, Future};
use report::FailedReport;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub use report::NELReport;

pub const NEL_ENDPOINT: &'static str = "a.nel.cloudflare.com";
const RETRY_TIMEOUT: Duration = Duration::from_secs(5);

lazy_static! {
    static ref QUERY_STRING: Mutex<String> = Mutex::new("".to_string());
    static ref REPORT_QUEUE: Queue<NELReport> = Queue::new(256);
}

// submit_report adds a report to the queue to be sent to the server.
pub fn submit_report(report: NELReport) {
    let _ = REPORT_QUEUE.try_push(report);
}

#[derive(Serialize, Deserialize)]
struct ReportToHeader {
    group: String,
    endpoints: Vec<ReportEndpoint>,
}

#[derive(Serialize, Deserialize)]
struct ReportEndpoint {
    url: String,
}

// report_to takes the value of the Report-To header and saves the NEL endpoint URL.
pub fn report_to(hdr: &str) {
    let val = serde_json::from_str::<ReportToHeader>(hdr);
    if !val.is_ok() {
        return;
    }
    let val = val.unwrap();
    if val.group != "cf-nel" {
        return;
    } else if val.endpoints.len() < 1 {
        return;
    }

    let prefix = format!("https://{}/report?s=", NEL_ENDPOINT);
    for endpoint in val.endpoints {
        let query = endpoint.url.strip_prefix(&prefix);
        if query.is_none() {
            continue;
        }
        let query = query.unwrap();

        let mut query_string = QUERY_STRING.lock().unwrap();
        *query_string = query.to_string();
        return;
    }
}

// handle_reports receives NEL reports and submits them to the reporting endpoint.
pub async fn handle_reports<F, G, FFut, GFut>(sleep: F, post: G)
where
    F: Fn(Duration) -> FFut,
    G: Fn(String, String, String) -> GFut,
    FFut: Future<Output = ()>,
    GFut: Future<Output = bool>,
{
    let pop = REPORT_QUEUE.pop().fuse();

    let failed_queue: Queue<FailedReport> = Queue::new(256);
    let fail_timeout = Fuse::terminated();
    let mut next_failed: Option<FailedReport> = None;

    pin_mut!(pop, fail_timeout);

    loop {
        select! {
            report = pop => {
                // Submit report.
                let payload = report.serialize();
                let path = {
                    format!("/report?s={}", QUERY_STRING.lock().unwrap())
                };
                let success = post(NEL_ENDPOINT.to_string(), path, payload).await;

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
                let payload = next_failed.as_ref().unwrap().original.serialize();
                let path = {
                    format!("/report?s={}", QUERY_STRING.lock().unwrap())
                };
                let success = post(NEL_ENDPOINT.to_string(), path, payload).await;

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
                        .unwrap_or(Duration::from_millis(10));
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
