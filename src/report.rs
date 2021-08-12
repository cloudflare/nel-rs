use crate::error::Error;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// NELReport captures all of the internal information we need about an error that occurred.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NELReport {
    captured: Instant,

    pub url: String,
    pub server_ip: String,
    pub protocol: String,
    pub method: String,
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
            status_code: 0,
            phase: "".to_string(),
            error_type: "".to_string(),
        }
    }

    pub fn set_server_ip<T: ToString>(&mut self, val: Option<T>) {
        let mut server_ip = opt_to_string(val);

        // Remove port if present.
        if server_ip.starts_with("[") {
            server_ip = server_ip[1..].splitn(2, ']').next().unwrap().to_string();
        } else {
            server_ip = server_ip.splitn(2, ':').next().unwrap().to_string();
        }

        self.server_ip = server_ip;
    }
    pub fn set_protocol<T: ToString>(&mut self, val: Option<T>) {
        self.protocol = opt_to_string(val);
    }
    pub fn set_method<T: ToString>(&mut self, val: Option<T>) {
        self.method = opt_to_string(val);
    }
    pub fn set_error<T: Into<Error>>(&mut self, err: T) {
        let mut err: Error = err.into();
        if self.protocol == "wireguard" && err.class == "tcp" {
            err.class = "udp".to_string();
        }
        self.phase = err.phase();
        self.error_type = err.to_string();
    }
    pub fn set_status_code(&mut self, code: usize) {
        self.status_code = code;
    }

    pub fn serialize(&self) -> String {
        let hdrs = vec![ReportHeader::from(self)];
        serde_json::to_string(&hdrs).unwrap()
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
    referrer: String,
    sampling_fraction: f64,
    server_ip: String,
    protocol: String,
    method: String,
    status_code: usize,
    elapsed_time: usize,
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
                referrer: "".to_string(),
                sampling_fraction: 1.0,
                server_ip: report.server_ip.clone(),
                protocol: report.protocol.clone(),
                method: report.method.clone(),
                status_code: report.status_code,
                elapsed_time: 0,
                phase: report.phase.to_string(),
                error_type: report.error_type.clone(),
            },
        }
    }
}
