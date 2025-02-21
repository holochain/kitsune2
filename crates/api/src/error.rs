//! Kitsune2 error types.

use std::sync::Arc;

/// A clonable trait-object inner error.
#[derive(Clone, Default)]
pub struct DynInnerError(
    pub Option<Arc<dyn std::error::Error + 'static + Send + Sync>>,
);

impl std::fmt::Debug for DynInnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::fmt::Display for DynInnerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0.as_ref() {
            None => f.write_str("None"),
            Some(s) => s.fmt(f),
        }
    }
}

impl std::error::Error for DynInnerError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.0.as_ref().map(|s| {
            let out: &(dyn std::error::Error + 'static) = &**s;
            out
        })
    }
}

impl DynInnerError {
    /// Construct a new DynInnerError from a source error.
    pub fn new<E: std::error::Error + 'static + Send + Sync>(e: E) -> Self {
        Self(Some(Arc::new(e)))
    }
}

/// The core kitsune2 error type. This type is used in all external
/// kitsune apis as well as internally in some modules.
///
/// This type is required to implement `Clone` to ease the use of
/// shared futures, which require the entire `Result` to be `Clone`.
#[derive(Debug, Clone, thiserror::Error)]
pub enum K2Error {
    /// Generic Kitsune2 internal error.
    #[error("{ctx} (src: {src})")]
    Other {
        /// Any context associated with this error.
        ctx: Arc<str>,

        /// The inner error (if any).
        #[source]
        src: DynInnerError,
    },
}

impl K2Error {
    /// Construct an "other" error with an inner source error.
    pub fn other_src<
        C: std::fmt::Display,
        S: std::error::Error + 'static + Send + Sync,
    >(
        ctx: C,
        src: S,
    ) -> Self {
        Self::Other {
            ctx: ctx.to_string().into_boxed_str().into(),
            src: DynInnerError::new(src),
        }
    }

    /// Construct an "other" error.
    pub fn other<C: std::fmt::Display>(ctx: C) -> Self {
        Self::Other {
            ctx: ctx.to_string().into_boxed_str().into(),
            src: DynInnerError::default(),
        }
    }
}

/// The core kitsune2 result type.
pub type K2Result<T> = Result<T, K2Error>;

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn error_display() {
        assert_eq!(
            "bla (src: None)",
            K2Error::other("bla").to_string().as_str(),
        );
        assert_eq!(
            "bla (src: None)",
            K2Error::other("bla".to_string()).to_string().as_str(),
        );
        assert_eq!(
            "foo (src: bar)",
            K2Error::other_src("foo", std::io::Error::other("bar"))
                .to_string()
                .as_str(),
        );
    }

    #[test]
    fn error_debug() {
        assert_eq!(
            "Other { ctx: \"bla\", src: None }",
            format!("{:?}", K2Error::other("bla")).as_str(),
        );
        assert_eq!(
            "Other { ctx: \"foo\", src: Some(Custom { kind: Other, error: \"bar\" }) }",
            format!(
                "{:?}",
                K2Error::other_src("foo", std::io::Error::other("bar"))
            )
            .as_str(),
        );
    }

    #[test]
    fn ensure_k2error_type_is_send_and_sync() {
        fn ensure<T: std::fmt::Display + Send + Sync>(_t: T) {}
        ensure(K2Error::other("bla"));
    }
}
