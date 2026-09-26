//! Annotation access control.
//!
//! Saving a location requires the password in `ANNOTATE_PASSWORD`. Without
//! it, saves are refused in release builds (the deployed app) so an
//! unconfigured deployment is read-only, and allowed in debug builds so local
//! `dx serve` editing needs no setup.

pub fn check_annotate_password(given: &str) -> Result<(), String> {
    match std::env::var("ANNOTATE_PASSWORD") {
        Ok(expected) if !expected.is_empty() => {
            if constant_time_eq(given.as_bytes(), expected.as_bytes()) {
                Ok(())
            } else {
                Err("wrong annotation password".into())
            }
        }
        _ if cfg!(debug_assertions) => Ok(()),
        _ => Err("annotation is disabled on this server (ANNOTATE_PASSWORD is not set)".into()),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::constant_time_eq;

    #[test]
    fn compares_bytes() {
        assert!(constant_time_eq(b"secret", b"secret"));
        assert!(!constant_time_eq(b"secret", b"secreT"));
        assert!(!constant_time_eq(b"secret", b"secrets"));
        assert!(constant_time_eq(b"", b""));
    }
}
