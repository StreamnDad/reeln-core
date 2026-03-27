use crate::MediaError;

/// A filter chain builder for constructing libav* filter graphs.
#[derive(Debug, Default, Clone)]
pub struct FilterChain {
    filters: Vec<String>,
}

impl FilterChain {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, filter: &str) -> &mut Self {
        self.filters.push(filter.to_string());
        self
    }

    /// Return the number of filters in the chain.
    pub fn len(&self) -> usize {
        self.filters.len()
    }

    /// Return true if the chain has no filters.
    pub fn is_empty(&self) -> bool {
        self.filters.is_empty()
    }

    /// Build the filter graph string for libav*.
    pub fn build(&self) -> Result<String, MediaError> {
        if self.filters.is_empty() {
            return Err(MediaError::Filter("empty filter chain".to_string()));
        }
        Ok(self.filters.join(","))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_chain_is_empty() {
        let chain = FilterChain::new();
        assert!(chain.is_empty());
        assert_eq!(chain.len(), 0);
    }

    #[test]
    fn test_default_chain_is_empty() {
        let chain = FilterChain::default();
        assert!(chain.is_empty());
    }

    #[test]
    fn test_add_single_filter() {
        let mut chain = FilterChain::new();
        chain.add("scale=1920:1080");
        assert_eq!(chain.len(), 1);
        assert!(!chain.is_empty());
        assert_eq!(chain.build().unwrap(), "scale=1920:1080");
    }

    #[test]
    fn test_add_multiple_filters() {
        let mut chain = FilterChain::new();
        chain.add("scale=1920:1080");
        chain.add("fps=30");
        chain.add("format=yuv420p");
        assert_eq!(chain.len(), 3);
        assert_eq!(
            chain.build().unwrap(),
            "scale=1920:1080,fps=30,format=yuv420p"
        );
    }

    #[test]
    fn test_build_empty_chain_returns_error() {
        let chain = FilterChain::new();
        let result = chain.build();
        assert!(result.is_err());
        match result.unwrap_err() {
            MediaError::Filter(msg) => assert!(msg.contains("empty")),
            other => panic!("expected Filter error, got {other:?}"),
        }
    }

    #[test]
    fn test_chained_add_calls() {
        let mut chain = FilterChain::new();
        chain.add("a").add("b").add("c");
        assert_eq!(chain.build().unwrap(), "a,b,c");
    }

    #[test]
    fn test_clone() {
        let mut chain = FilterChain::new();
        chain.add("scale=640:480");
        let cloned = chain.clone();
        assert_eq!(cloned.build().unwrap(), "scale=640:480");
    }

    #[test]
    fn test_debug_format() {
        let chain = FilterChain::new();
        let debug = format!("{chain:?}");
        assert!(debug.contains("FilterChain"));
    }

    #[test]
    fn test_add_complex_filter() {
        let mut chain = FilterChain::new();
        chain.add("drawtext=text='hello world':fontsize=24:x=10:y=10");
        assert_eq!(
            chain.build().unwrap(),
            "drawtext=text='hello world':fontsize=24:x=10:y=10"
        );
    }
}
