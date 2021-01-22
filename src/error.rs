use rustls::TLSError;
use std::string::ToString;

pub struct Error {
    class: String,
    subclass: String,
}

impl Error {
    fn new(class: &str, subclass: &str) -> Error {
        Error {
            class: class.to_string(),
            subclass: subclass.to_string(),
        }
    }

    pub fn phase(&self) -> String {
        match self.class.as_ref() {
            "dns" => "dns",
            "tcp" => "connection",
            "tls" => "connection",
            "http" => "application",
            _ => "unknown",
        }
        .to_string()
    }
}

impl ToString for Error {
    fn to_string(&self) -> String {
        if self.class == "unknown" {
            "unknown".to_string()
        } else {
            format!("{}.{}", self.class, self.subclass)
        }
    }
}

impl From<&std::io::Error> for Error {
    fn from(err: &std::io::Error) -> Self {
        use std::io::ErrorKind;

        match err.kind() {
            ErrorKind::TimedOut => Error::new("tcp", "timed_out"),
            ErrorKind::ConnectionReset => Error::new("tcp", "reset"),
            ErrorKind::ConnectionRefused => Error::new("tcp", "refused"),
            ErrorKind::ConnectionAborted => Error::new("tcp", "aborted"),
            _ => match err.get_ref() {
                None => Error::new("tcp", "failed"),
                Some(inner) => {
                    if inner.downcast_ref::<TLSError>().is_some() {
                        Error::new("tls", "protocol.error")
                    } else {
                        Error::new("unknown", "unknown")
                    }
                }
            },
        }
    }
}
