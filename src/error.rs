use std::string::ToString;

pub struct Error {
    class: String,
    subclass: String,
}

impl Error {
    fn new(class: String, subclass: String) -> Error {
        Error {
            class: class,
            subclass: subclass,
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

// impl From<std::io::Error> for Error {
//     fn from(err: std::io::Error) -> Self {
//         Error::new("unknown".to_string(), "unknown".to_string())
//     }
// }
