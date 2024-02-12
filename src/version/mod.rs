/// Represents the Magi version
#[derive(Debug)]
pub struct Version {
    /// The package name specified in `Cargo.toml`
    name: String,
    /// The package version specified in `Cargo.toml`
    version: String,
    /// `Dev` if compiled in debug mode. `Release` otherwise.
    meta: String,
}

impl Version {
    /// Build and returns a [Version] struct
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
    /// Formatted as: {name}{version}-{meta}
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
