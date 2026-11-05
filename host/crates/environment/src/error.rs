#[derive(Debug)]
pub enum EnvironmentError {
    Invalid(String),
    Io(std::io::Error),
}
impl From<std::io::Error> for EnvironmentError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}
impl std::fmt::Display for EnvironmentError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Invalid(message) => formatter.write_str(message),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}
impl std::error::Error for EnvironmentError {}
