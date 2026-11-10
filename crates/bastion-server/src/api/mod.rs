//! What each client may do, one module per resource (D28). The HTTP routes
//! call these.

pub mod console;
pub mod costs;
pub mod doctor;
pub mod engine;
pub mod evals;
pub mod factory;
pub mod govern;
pub mod hosts;
pub mod people;
pub mod providers;
pub mod sessions;
pub mod settings;
pub mod sign_in;
pub mod stack;
pub mod workflows;

/// Which part of a long list to answer with: `limit` items after skipping
/// `offset`, newest first.
#[derive(Debug, Default, serde::Deserialize)]
pub struct Page {
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

impl Page {
    const DEFAULT: usize = 200;
    const MOST: usize = 1000;

    /// The limit and offset, with the default limit when none is given and
    /// no limit above the most.
    pub fn bounds(&self) -> (usize, usize) {
        let limit = self.limit.unwrap_or(Self::DEFAULT).min(Self::MOST);
        (limit, self.offset.unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::Page;

    #[test]
    fn a_page_has_a_default_limit_and_a_most() {
        assert_eq!(Page::default().bounds(), (200, 0));
        let asked = Page {
            limit: Some(5000),
            offset: Some(30),
        };
        assert_eq!(asked.bounds(), (1000, 30));
    }
}
