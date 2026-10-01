//! Identity of compiled crates and the source of each model.
//!
//! A [`BuildInfo`] names one compiled crate: its package, its version and, once a build script stamps it, its
//! commit, a dirty flag and a hash of its sources. [`build_info!`](crate::build_info) returns the one of the crate it
//! expands in. A [`ModelSource`] names the origin of a model's code: the type registered, and the build of the
//! crate that registered it.

use std::borrow::Cow;

/// Identity of one compiled crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildInfo {
    package: &'static str,
    version: &'static str,
    commit: &'static str,
    commit_date: &'static str,
    dirty: Option<bool>,
    source_hash: Option<u64>,
    debug_build: bool,
}

impl BuildInfo {
    /// Returns the build that the environment of [`build_info!`](crate::build_info) describes.
    ///
    /// `dirty` reads `true` or `false`, and `source_hash` 16 lowercase hexadecimal digits. Any other text reads as
    /// unknown.
    #[doc(hidden)]
    pub const fn __from_env(
        package: &'static str,
        version: &'static str,
        commit: Option<&'static str>,
        commit_date: Option<&'static str>,
        dirty: Option<&'static str>,
        source_hash: Option<&'static str>,
        debug_build: bool,
    ) -> Self {
        Self {
            package,
            version,
            commit: match commit {
                Some(commit) => commit,
                None => "",
            },
            commit_date: match commit_date {
                Some(date) => date,
                None => "",
            },
            dirty: match dirty {
                Some(text) => parse_flag(text),
                None => None,
            },
            source_hash: match source_hash {
                Some(text) => parse_hash(text),
                None => None,
            },
            debug_build,
        }
    }

    pub fn package(&self) -> &'static str {
        self.package
    }

    pub fn version(&self) -> &'static str {
        self.version
    }

    /// Short commit hash, empty when the build could not learn it.
    pub fn commit(&self) -> &'static str {
        self.commit
    }

    /// Commit date, empty in a registry or vendored build.
    pub fn commit_date(&self) -> &'static str {
        self.commit_date
    }

    /// Whether the crate's sources, manifest or lockfile differed from its commit. `None` when the build could not
    /// tell.
    pub fn dirty(&self) -> Option<bool> {
        self.dirty
    }

    /// Hash of the crate's sources, manifest and lockfile. `None` when the build did not compute one.
    pub fn source_hash(&self) -> Option<u64> {
        self.source_hash
    }

    /// Whether the crate was compiled with debug assertions.
    pub fn debug_build(&self) -> bool {
        self.debug_build
    }
}

/// Returns `true` for "true", `false` for "false", and `None` for any other text.
const fn parse_flag(text: &str) -> Option<bool> {
    match text.as_bytes() {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}

/// Returns the value of 16 lowercase hexadecimal digits, or `None` for any other text.
const fn parse_hash(text: &str) -> Option<u64> {
    let bytes = text.as_bytes();
    if bytes.len() != 16 {
        return None;
    }
    let mut value = 0_u64;
    let mut index = 0;
    while index < bytes.len() {
        let digit = match bytes[index] {
            byte @ b'0'..=b'9' => byte - b'0',
            byte @ b'a'..=b'f' => byte - b'a' + 10,
            _ => return None,
        };
        value = (value << 4) | digit as u64;
        index += 1;
    }
    Some(value)
}

/// Returns the [`BuildInfo`] of the crate the macro expands in.
///
/// The package and version come from Cargo. The commit, its date, the dirty flag and the source hash come from
/// variables a build script sets, and read as unknown where none does.
#[macro_export]
macro_rules! build_info {
    () => {
        $crate::provenance::BuildInfo::__from_env(
            ::core::env!("CARGO_PKG_NAME"),
            ::core::env!("CARGO_PKG_VERSION"),
            ::core::option_env!("HENAD_BUILD_COMMIT"),
            ::core::option_env!("HENAD_BUILD_COMMIT_DATE"),
            ::core::option_env!("HENAD_BUILD_DIRTY"),
            ::core::option_env!("HENAD_BUILD_SOURCE_HASH"),
            ::core::cfg!(debug_assertions),
        )
    };
}

/// Origin of a model's code.
///
/// Registration records the model's type path. The registering crate's build is recorded once the entry joins a
/// model set, and the accessors that read it return empty text or `None` until then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSource {
    type_path: Cow<'static, str>,
    build: Option<BuildInfo>,
}

impl ModelSource {
    /// Returns a source that records `type_path` and no build yet.
    #[doc(hidden)]
    pub fn __from_type_path(type_path: &'static str) -> Self {
        Self {
            type_path: Cow::Borrowed(type_path),
            build: None,
        }
    }

    /// Returns the source with `build` recorded as the registering crate's.
    #[doc(hidden)]
    pub fn __with_build(self, build: BuildInfo) -> Self {
        Self {
            build: Some(build),
            ..self
        }
    }

    /// Build of the registering crate, `None` until a set records it.
    pub fn build(&self) -> Option<&BuildInfo> {
        self.build.as_ref()
    }

    /// Package of the registering crate, empty until a set records its build.
    pub fn package(&self) -> &str {
        self.build.as_ref().map_or("", BuildInfo::package)
    }

    pub fn version(&self) -> &str {
        self.build.as_ref().map_or("", BuildInfo::version)
    }

    pub fn commit(&self) -> &str {
        self.build.as_ref().map_or("", BuildInfo::commit)
    }

    /// Whether the registering crate's sources, manifest or lockfile differed from its commit, as
    /// [`BuildInfo::dirty`] reads it.
    pub fn dirty(&self) -> Option<bool> {
        self.build.as_ref().and_then(BuildInfo::dirty)
    }

    /// Hash of the registering crate's files, as [`BuildInfo::source_hash`] reads it.
    pub fn source_hash(&self) -> Option<u64> {
        self.build.as_ref().and_then(BuildInfo::source_hash)
    }

    /// Type path of the registered model, from `std::any::type_name`. For reading only.
    ///
    /// Note that the format of a type path is not stable across compiler releases, so no check compares it.
    pub fn type_path(&self) -> &str {
        &self.type_path
    }
}

#[cfg(test)]
mod tests {
    use super::{BuildInfo, ModelSource};

    #[test]
    fn an_unstamped_build_reads_as_unknown() {
        let build = crate::build_info!();
        assert_eq!(build.package(), "henad-core");
        assert_eq!(build.version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(build.commit(), "");
        assert_eq!(build.dirty(), None);
        assert_eq!(build.source_hash(), None);
        assert_eq!(build.debug_build(), cfg!(debug_assertions));
    }

    #[test]
    fn a_stamped_build_reads_its_flag_and_hash() {
        let build = BuildInfo::__from_env(
            "pkg",
            "1.0.0",
            Some("abc1234"),
            Some("2026-10-01"),
            Some("true"),
            Some("00000000deadbeef"),
            false,
        );
        assert_eq!(build.commit(), "abc1234");
        assert_eq!(build.commit_date(), "2026-10-01");
        assert_eq!(build.dirty(), Some(true));
        assert_eq!(build.source_hash(), Some(0xDEAD_BEEF));
    }

    #[test]
    fn malformed_stamps_read_as_unknown() {
        let build = BuildInfo::__from_env("pkg", "1.0.0", None, None, Some("yes"), Some("DEADBEEF"), false);
        assert_eq!(build.dirty(), None);
        assert_eq!(build.source_hash(), None);
    }

    #[test]
    fn a_source_reads_its_build_once_recorded() {
        let source = ModelSource::__from_type_path("crate::Model");
        assert_eq!((source.package(), source.type_path()), ("", "crate::Model"));
        let build = BuildInfo::__from_env("pkg", "1.0.0", Some("abc1234"), None, None, None, false);
        let source = source.__with_build(build);
        assert_eq!(
            (source.package(), source.version(), source.commit()),
            ("pkg", "1.0.0", "abc1234")
        );
        assert_eq!(source.build(), Some(&build));
    }
}
