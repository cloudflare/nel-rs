impl From<&reqwest::Error> for super::Error {
    fn from(err: &reqwest::Error) -> Self {
        use std::error::Error;

        // First, attempt to trace this to an underlying hyper::Error
        let mut source = err.source();
        while let Some(err) = source {
            if let Some(hyper_err) = err.downcast_ref::<hyper::Error>() {
                return hyper_err.into();
            }

            source = err.source();
        }

        super::Error::new("unknown", err)
    }
}

impl From<&hyper::Error> for super::Error {
    fn from(err: &hyper::Error) -> Self {
        use std::error::Error;

        // If this is caused by an underlying I/O error, delegate to that.
        let mut source = err.source();
        while let Some(err) = source {
            if let Some(io_err) = err.downcast_ref::<std::io::Error>() {
                return io_err.into();
            }

            source = err.source();
        }

        // Many of these matches contain a std::io::Error as an inner, but we want
        // to handle them specially.
        if err.is_connect() {
            // this was an error from `Connect`. Could be any number of things.
            if let Some(source) = err.source() {
                match source.to_string() {
                    s if s.contains("Hostname mismatch") => {
                        super::Error::new("tls", "cert.name_invalid")
                    }
                    s if s.contains("certificate has expired") => {
                        super::Error::new("tls", "cert.date_invalid")
                    }
                    s if s.contains("self signed certificate in certificate chain") => {
                        super::Error::new("tls", "cert.authority_invalid")
                    }
                    _ => super::Error::new("tcp", "failed"),
                }
            } else {
                super::Error::new("tcp", "failed")
            }
        } else if err.is_parse() {
            // this was an HTTP parse error.
            super::Error::new("http", "response.invalid")
        } else if err.is_user() {
            // this error was caused by user code.
            super::Error::new("http", "protocol.error")
        } else if err.is_incomplete_message() {
            // the connection closed before a message could complete.
            super::Error::new("tcp", "closed")
        } else if err.is_body_write_aborted() {
            // the body write was aborted.
            super::Error::new("abandoned", "")
        } else if err.is_timeout() {
            super::Error::new("tcp", "timed_out")
        } else if err.is_closed() {
            // a sender's channel was closed.
            super::Error::new("tcp", "reset")
        } else if err.is_canceled() {
            // the `Request` was canceled.
            super::Error::new("tcp", "aborted")
        } else {
            super::Error::new("unknown", err)
        }
    }
}

#[cfg(test)]
mod tests {
    // We can't get all of them, but at least here are the most common DNS and TLS failures.
    // We test them with both rustls and native-tls, because the errors are completely disjoint.

    fn clients() -> Vec<reqwest::Client> {
        vec![
            reqwest::ClientBuilder::new()
                .use_native_tls()
                .build()
                .unwrap(),
            reqwest::ClientBuilder::new()
                .use_rustls_tls()
                .build()
                .unwrap(),
        ]
    }

    #[tokio::test]
    async fn no_dns() {
        for client in clients() {
            let response = client.get("http://invalid.").send().await.unwrap_err();
            let nel_err: crate::error::Error = (&response).into();
            assert_eq!(nel_err.to_string(), "dns.name_not_resolved");
            assert_eq!(nel_err.phase(), "dns");
        }
    }

    #[tokio::test]
    async fn expired_cert() {
        for client in clients() {
            let response = client
                .get("https://expired.badssl.com/")
                .send()
                .await
                .unwrap_err();
            let nel_err: crate::error::Error = (&response).into();
            assert_eq!(nel_err.to_string(), "tls.cert.date_invalid");
            assert_eq!(nel_err.phase(), "connection");
        }
    }

    #[tokio::test]
    async fn untrusted_cert() {
        for client in clients() {
            let response = client
                .get("https://untrusted-root.badssl.com/")
                .send()
                .await
                .unwrap_err();
            let nel_err: crate::error::Error = (&response).into();
            assert_eq!(nel_err.to_string(), "tls.cert.authority_invalid");
            assert_eq!(nel_err.phase(), "connection");
        }
    }

    #[tokio::test]
    async fn invalid_name() {
        for client in clients() {
            let response = client
                .get("https://wrong.host.badssl.com/")
                .send()
                .await
                .unwrap_err();
            let nel_err: crate::error::Error = (&response).into();
            assert_eq!(nel_err.to_string(), "tls.cert.name_invalid");
            assert_eq!(nel_err.phase(), "connection");
        }
    }
}
