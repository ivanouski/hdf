use std::fmt::Display;

// Turns an error into a message for the user, after what failed: "cannot delete: <error>"
pub trait Context<T> {
    fn context(self, prefix: impl Display) -> Result<T, String>;
}

impl<T, E: Display> Context<T> for Result<T, E> {
    fn context(self, prefix: impl Display) -> Result<T, String> {
        self.map_err(|error| format!("{prefix}: {error}"))
    }
}
