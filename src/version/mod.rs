#[derive(Debug)]
pub struct Version {
    name: String,
    version: String,
    meta: String,
}

impl Version {
    pub fn build() -> Self {
        let meta = if cfg!(debug_assertions) {
            "dev"
        } else {
            "release"
        };

        Version {
            name: env!("CARGO_PKG_NAME").to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            meta: meta.to_string(),
        }
    }
}

impl ToString for Version {
    fn to_string(&self) -> String {
        format!("{}{}-{}", self.name, self.version, self.meta)
    }
}

#[cfg(test)]
mod tests {
    use crate::version::Version;

    #[test]
    fn version() {
        let version = Version::build();
        assert!(version.to_string() == "magi0.1.0-dev");
    }
}
