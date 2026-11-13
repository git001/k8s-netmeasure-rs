//! Build information, reported by `--version` and later in every result file (§5).

/// Values fixed at build time by `build.rs`.
pub struct BuildInfo {
    pub version: &'static str,
    pub git_sha: &'static str,
    pub rustc: &'static str,
    pub target: &'static str,
    pub is_static: bool,
}

pub const BUILD_INFO: BuildInfo = BuildInfo {
    version: env!("CARGO_PKG_VERSION"),
    git_sha: env!("NETMEASURE_BUILD_GIT_SHA"),
    rustc: env!("NETMEASURE_BUILD_RUSTC"),
    target: env!("NETMEASURE_BUILD_TARGET"),
    is_static: cfg!(target_env = "musl"),
};

impl BuildInfo {
    /// The `build_info` object of result and report files (§5).
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": self.version, "git_sha": self.git_sha, "rustc": self.rustc,
            "target": self.target, "static": self.is_static,
        })
    }

    /// One line for `--version`, e.g.
    /// `0.1.0 (git 0123abcd, rustc 1.100.0, x86_64-unknown-linux-gnu, dynamic)`.
    pub fn version_line(&self) -> String {
        let linkage = if self.is_static { "static" } else { "dynamic" };
        format!(
            "{} (git {}, rustc {}, {}, {})",
            self.version, self.git_sha, self.rustc, self.target, linkage
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_every_field() {
        let info = BuildInfo {
            version: "1.2.3",
            git_sha: "abc",
            rustc: "1.100.0",
            target: "x86_64-unknown-linux-musl",
            is_static: true,
        };
        assert_eq!(
            info.version_line(),
            "1.2.3 (git abc, rustc 1.100.0, x86_64-unknown-linux-musl, static)"
        );
    }

    #[test]
    fn build_info_matches_package_version() {
        assert_eq!(BUILD_INFO.version, env!("CARGO_PKG_VERSION"));
        assert!(!BUILD_INFO.git_sha.is_empty());
    }
}
