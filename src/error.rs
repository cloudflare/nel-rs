#[cfg(feature = "reqwest-error")]
mod reqwest;

#[cfg(feature = "reqwest-error")]
pub use self::reqwest::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Error {
    pub class: String,
    pub subclass: String,
}

impl Error {
    fn new<C, S>(class: C, subclass: S) -> Error
    where
        C: std::fmt::Display,
        S: std::fmt::Display,
    {
        Error {
            class: class.to_string(),
            subclass: subclass.to_string(),
        }
    }

    pub fn phase(&self) -> String {
        match self.class.as_ref() {
            "dns" => "dns",
            "tcp" => "connection",
            "udp" => "connection",
            "tls" => "connection",
            "http" => "application",
            "abandoned" => "application",
            _ => "unknown",
        }
        .to_string()
    }
}

impl ToString for Error {
    fn to_string(&self) -> String {
        if self.class == "unknown" {
            "unknown".to_string()
        } else if self.class == "abandoned" {
            "abandoned".to_string()
        } else {
            format!("{}.{}", self.class, self.subclass)
        }
    }
}

impl From<&std::io::Error> for Error {
    fn from(err: &std::io::Error) -> Self {
        use std::io::ErrorKind;

        eprintln!("{:?}", err);

        match err.kind() {
            ErrorKind::TimedOut => Error::new("tcp", "timed_out"),
            ErrorKind::ConnectionReset => Error::new("tcp", "reset"),
            ErrorKind::ConnectionRefused => Error::new("tcp", "refused"),
            ErrorKind::ConnectionAborted => Error::new("tcp", "aborted"),

            _ => match err.to_string().to_lowercase() {
                str if str.contains("no address") || str.contains("name or service not known") => {
                    Error::new("dns", "name_not_resolved")
                }
                str if str.contains("no route to host") => Error::new("tcp", "address_unreachable"),
                str if str.contains("unreachable") => Error::new("tcp", "address_unreachable"),
                str if str.contains("expired") => Error::new("tls", "cert.date_invalid"),
                str if str.contains("unknownissuer") => Error::new("tls", "cert.authority_invalid"),
                str if str.contains("certnotvalidforname") => {
                    Error::new("tls", "cert.name_invalid")
                }
                _ => match err.get_ref() {
                    None => Error::new("tcp", "failed"),
                    Some(inner) => {
                        if inner.downcast_ref::<rustls::Error>().is_some() {
                            Error::new("tls", "protocol.error")
                        } else {
                            Error::new("unknown", err)
                        }
                    }
                },
            },
        }
    }
}
