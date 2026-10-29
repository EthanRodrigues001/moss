use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("no TeX installation with latexmk was found")]
    TexNotFound,
    #[error("latexmk needs Perl, but none was found")]
    PerlNotFound,
    #[error("root file `{0}` does not exist in the project")]
    RootFileMissing(String),
    #[error("path `{0}` escapes the project folder or is not allowed")]
    UnsafePath(String),
    #[error("`{name}` is not a valid Windows name: {reason}")]
    InvalidName { name: String, reason: String },
    #[error("project `{0}` was not found")]
    ProjectNotFound(String),
    #[error("`{0}` is not a Moss project (missing .moss/project.json)")]
    NotAProject(PathBuf),
    #[error("{0} is not valid UTF-8 text")]
    NotUtf8(String),
    #[error("could not read zip: {0}")]
    Zip(String),
    #[error("not signed in to Overleaf (the session is missing or has expired)")]
    OverleafAuth,
    #[error("Overleaf: {0}")]
    Overleaf(String),
    #[error("history: {0}")]
    History(String),
    #[error("approval: {0}")]
    Approval(String),
    #[error("{0}")]
    Entry(String),
    #[error("sync: {0}")]
    Sync(String),
    #[error("compile: {0}")]
    Compile(String),
    #[error("pdf: {0}")]
    Pdf(String),
    #[error("setup: {0}")]
    Setup(String),
    #[error("cancelled")]
    Cancelled,
    #[error("{context}: {source}")]
    Io {
        context: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{context}: {source}")]
    Json {
        context: String,
        #[source]
        source: serde_json::Error,
    },
}

impl Error {
    pub fn io(context: impl Into<String>, source: std::io::Error) -> Self {
        Error::Io {
            context: context.into(),
            source,
        }
    }

    pub fn json(context: impl Into<String>, source: serde_json::Error) -> Self {
        Error::Json {
            context: context.into(),
            source,
        }
    }
}
