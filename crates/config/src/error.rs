use std::fmt;

pub type Result<T> = std::result::Result<T, ConfigError>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigErrorKind {
    InvalidDocument,
    InvalidToml,
    InvalidField,
    InvalidPath,
    Io,
    Serialize,
    UnsupportedCredentialSource,
}

/// Never retains parser excerpts, configuration values, URLs, or OS error text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub kind: ConfigErrorKind,
    pub field: Option<String>,
}

impl ConfigError {
    pub(crate) fn new(kind: ConfigErrorKind) -> Self {
        Self { kind, field: None }
    }

    pub(crate) fn field(name: &str) -> Self {
        Self {
            kind: ConfigErrorKind::InvalidField,
            field: Some(name.into()),
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.kind {
            ConfigErrorKind::InvalidDocument => f.write_str("configuration must be a table"),
            ConfigErrorKind::InvalidToml => f.write_str("configuration is not valid TOML"),
            ConfigErrorKind::InvalidField => {
                f.write_str("invalid configuration field")?;
                if let Some(field) = &self.field { write!(f, ": {field}")?; }
                Ok(())
            }
            ConfigErrorKind::InvalidPath => f.write_str("application directory must be an absolute path"),
            ConfigErrorKind::Io => f.write_str("configuration file operation failed"),
            ConfigErrorKind::Serialize => f.write_str("configuration cannot be represented in TOML"),
            ConfigErrorKind::UnsupportedCredentialSource => f.write_str("Tencent requires configured credentials; profile, CVM and TKE credential discovery are not supported"),
        }
    }
}

impl std::error::Error for ConfigError {}
