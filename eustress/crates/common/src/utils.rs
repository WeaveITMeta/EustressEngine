//! Shared utility functions

/// Generate a unique name with numeric suffix
pub fn unique_name(base: &str, existing: &[String]) -> String {
    let mut counter = 1;
    loop {
        let name = if counter == 1 {
            base.to_string()
        } else {
            format!("{} {}", base, counter)
        };
        
        if !existing.contains(&name) {
            return name;
        }
        counter += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unique_name() {
        let existing = vec!["Cube".to_string(), "Cube 2".to_string()];
        assert_eq!(unique_name("Cube", &existing), "Cube 3");
        assert_eq!(unique_name("Sphere", &existing), "Sphere");
    }
}
