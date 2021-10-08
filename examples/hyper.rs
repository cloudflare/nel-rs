const ENDPOINT: &str = "https://ivan.computer/";

lazy_static::lazy_static! {
    static ref NEL_CLIENT: hyper::Client<hyper_tls::HttpsConnector<hyper::client::connect::HttpConnector>> = hyper::Client::builder().build::<_, hyper::Body>(hyper_tls::HttpsConnector::new());
}

#[tokio::main(flavor = "current_thread")]
pub async fn main() {
    // Spawn a background future to drive reporting
    tokio::spawn(nel::handle_reports(tokio::time::sleep, post_to_cloudflare));

    let client = hyper::Client::builder().build::<_, hyper::Body>(hyper_tls::HttpsConnector::new());

    let req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(ENDPOINT)
        .body(hyper::Body::empty())
        .expect("failed to build request");

    // Send the first request to receive initial configuration in the response headers
    let _initial_resp = nel_wrapped_request(&client, req).await;

    let mut query_url = ENDPOINT.to_owned();
    query_url.push_str("dns-query");

    let req = hyper::Request::builder()
        .method(hyper::Method::GET)
        .uri(query_url)
        .body(hyper::Body::empty())
        .expect("failed to build request");

    // Send the second request to generate an error
    let _resp = nel_wrapped_request(&client, req).await;

    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
}

async fn post_to_cloudflare(uri: String, payload: String) -> bool {
    let req = hyper::Request::builder()
        .method(hyper::Method::POST)
        .uri(uri)
        .header(hyper::header::CONTENT_TYPE, "application/reports+json")
        .header(hyper::header::USER_AGENT, "example-hyper-nel-rs")
        .body(payload.into())
        .expect("failed to build nel request");

    let resp = NEL_CLIENT.request(req).await;

    eprintln!("nel report resp = {:?}", resp);

    // Whether this was successfully reported. Unsuccessful reports retried indefinitely
    // with a nel::RETRY_TIMEOUT sleep between them (5s at the time of writing).
    true
}

async fn nel_wrapped_request<C>(
    client: &hyper::Client<C, hyper::Body>,
    req: hyper::Request<hyper::Body>,
) -> hyper::Result<hyper::Response<hyper::Body>>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    let url = req.uri().clone();
    let method = req.method().clone();
    let resp = client.request(req).await;

    nel_process_response(method, url, &resp);

    resp
}

fn nel_process_response(
    method: hyper::Method,
    url: hyper::Uri,
    resp: &hyper::Result<hyper::Response<hyper::Body>>,
) {
    if let Ok(resp) = resp.as_ref() {
        let host = url.host().unwrap();
        for (name, value) in resp.headers() {
            if name == "nel" {
                nel::nel_header(host, value.to_str().expect("non-utf-8 nel header"))
            } else if name == "report-to" {
                nel::report_to_header(host, value.to_str().expect("non-utf-8 report-to header"));
            }
        }
    }

    match resp {
        Err(error) => report_error(method, url, 0, error.into()),
        Ok(resp) => {
            if resp.status() != 200 {
                // Cloudflare generally ignores "http.error", so we use "http.response.invalid"
                let error = nel::Error {
                    class: "http".to_owned(),
                    subclass: "response.invalid".to_owned(),
                };

                report_error(method, url, resp.status().as_u16() as usize, error);
            }
        }
    };
}

fn report_error(method: hyper::Method, url: hyper::Uri, status: usize, error: nel::Error) {
    let mut report = nel::NELReport::new(url.to_string());
    report.set_error(error);
    report.set_status_code(status);
    report.set_method(Some(method));

    nel::submit_report(report);
}
