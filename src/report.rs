use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// NELReport captures all of the internal information we need about an error that occurred.
#[derive(Debug)]
pub struct NELReport {
    captured: Instant,

    pub url: String,
    pub server_ip: String,
    pub protocol: String,
    pub method: String,
    pub request_headers: HashMap<String, Vec<String>>,
    pub response_headers: HashMap<String, Vec<String>>,
    pub status_code: usize,
    phase: String,
    error_type: String,
}

impl NELReport {
    pub fn new(url: String) -> Self {
        NELReport {
            captured: Instant::now(),

            url: url,
            server_ip: "".to_string(),
            protocol: "".to_string(),
            method: "".to_string(),
            request_headers: HashMap::new(),
            response_headers: HashMap::new(),
            status_code: 0,
            phase: "".to_string(),
            error_type: "".to_string(),
        }
    }

    pub fn set_server_ip<T: ToString>(&mut self, val: Option<T>) {
        self.server_ip = opt_to_string(val);
    }
    pub fn set_protocol<T: ToString>(&mut self, val: Option<T>) {
        self.protocol = opt_to_string(val);
    }
    pub fn set_method<T: ToString>(&mut self, val: Option<T>) {
        self.method = opt_to_string(val);
    }
    pub fn set_error<T: Into<Error>>(&mut self, err: T) {
        let err: Error = err.into();
        self.phase = err.phase();
        self.error_type = err.to_string();
    }

    pub fn serialize(&self) -> String {
        let hdr = ReportHeader::from(self);
        serde_json::to_string(&hdr).unwrap()
    }
}

fn opt_to_string<T: ToString>(input: Option<T>) -> String {
    match input {
        None => "".to_string(),
        Some(val) => val.to_string(),
    }
}

/// FailedReport wraps a report with the time we tried and failed to submit it to the NEL endpoint.
pub struct FailedReport {
    pub last_try: Instant,
    pub original: NELReport,
}

/// ReportHeader is the structure we serialize and submit to the NEL endpoint.
#[derive(Serialize, Deserialize)]
struct ReportHeader {
    age: usize,
    #[serde(rename = "type")]
    report_type: String,
    url: String,
    body: ReportBody,
}

#[derive(Serialize, Deserialize)]
struct ReportBody {
    sampling_fraction: f64,
    server_ip: String,
    protocol: String,
    method: String,
    request_headers: HashMap<String, Vec<String>>,
    response_headers: HashMap<String, Vec<String>>,
    status_code: usize,
    phase: String,
    #[serde(rename = "type")]
    error_type: String,
}

impl From<&NELReport> for ReportHeader {
    fn from(report: &NELReport) -> Self {
        ReportHeader {
            age: Instant::now()
                .checked_duration_since(report.captured)
                .unwrap_or(Duration::from_secs(0))
                .as_millis() as usize,
            report_type: "network-error".to_string(),
            url: report.url.clone(),
            body: ReportBody {
                sampling_fraction: 1.0,
                server_ip: report.server_ip.clone(),
                protocol: report.protocol.clone(),
                method: report.method.clone(),
                request_headers: report.request_headers.clone(),
                response_headers: report.response_headers.clone(),
                status_code: report.status_code,
                phase: report.phase.to_string(),
                error_type: report.error_type.clone(),
            },
        }
    }
}
