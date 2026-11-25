//! Research on the web (docs/spec/marketing-videos.md, ADR-0037): a model
//! reads a product's page and what is public about its market, and writes
//! notes that name the pages it read. The interface hides the provider's
//! protocol: the app runs no browser and no scraper, and what comes back
//! is text to review, never an instruction.

use std::sync::Arc;
use std::time::SystemTime;

use crate::{ApiKey, Metered, ProviderFailure, Stop, TextImage};

/// How far one research may go. The limits are sent with the request, so
/// a run cannot pass them and its cost has a ceiling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResearchLimits {
    /// Web searches, at most.
    pub searches: u32,
    /// Pages fetched, at most.
    pub fetches: u32,
    /// Tokens read from any one page, at most; the rest of a longer page
    /// is left out.
    pub page_tokens: u32,
}

impl ResearchLimits {
    /// The limits of a product research.
    pub const PRODUCT: ResearchLimits = ResearchLimits {
        searches: 8,
        fetches: 5,
        page_tokens: 20_000,
    };

    /// What one search's results are counted as when a run is estimated.
    /// The provider gives no limit for them; this is room for ten results
    /// with their excerpts.
    pub const SEARCH_RESULT_TOKENS: u64 = 6_000;

    /// How many times the model may be asked to go on: once to begin, and
    /// once after every search or fetch.
    pub fn steps(&self) -> u64 {
        u64::from(self.searches) + u64::from(self.fetches) + 1
    }

    /// The most tokens the run can add to what it was sent: every fetched
    /// page at its limit and every search's results.
    pub fn read_ceiling(&self) -> u64 {
        u64::from(self.fetches) * u64::from(self.page_tokens)
            + u64::from(self.searches) * Self::SEARCH_RESULT_TOKENS
    }

    /// The most tokens the run reads again: after each search or fetch the
    /// model is given everything before it once more, which the provider
    /// bills as cache reads when the request asks for its cache. Counted
    /// for the worst order, the largest results first, with `sent` tokens
    /// of instructions, prompt and pictures before them.
    pub fn reread_ceiling(&self, sent: u64) -> u64 {
        let page = u64::from(self.page_tokens);
        let mut results: Vec<u64> = std::iter::repeat_n(page, self.fetches as usize)
            .chain(std::iter::repeat_n(
                Self::SEARCH_RESULT_TOKENS,
                self.searches as usize,
            ))
            .collect();
        results.sort_unstable_by(|a, b| b.cmp(a));
        let mut held = sent;
        let mut reread = 0u64;
        for result in results {
            held = held.saturating_add(result);
            reread = reread.saturating_add(held);
        }
        reread
    }
}

/// What to research.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchRequest {
    /// Standing instructions: role, rules, what the notes must hold.
    pub instructions: String,
    /// The task, with what is known so far. A link the model should
    /// fetch has to be written here: providers fetch only addresses the
    /// request or an earlier result named.
    pub prompt: String,
    /// Pictures that go with the task, before it.
    pub images: Vec<TextImage>,
    pub limits: ResearchLimits,
}

/// A page the research read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRead {
    pub url: String,
    /// The page's own title; empty when it gave none.
    pub title: String,
    /// When the provider fetched it, when it says; a page the notes only
    /// cite from a search has no time of its own.
    pub read_at: Option<SystemTime>,
}

/// What a research wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchNotes {
    /// The notes, as the model wrote them.
    pub text: String,
    /// The pages it fetched, and the search results its notes cite, each
    /// once, in the order they were read.
    pub pages: Vec<PageRead>,
    /// The model that wrote the notes, for the record.
    pub model: String,
    /// Everything the run used, over every request it took: tokens sent,
    /// written and passed through the provider's cache, and the searches
    /// the provider counted.
    pub usage: Metered,
}

/// Researches on the web. Calls the network and blocks, so it runs inside
/// a job.
pub trait WebResearcher: Send + Sync {
    /// Researches, and gives up as soon as `stop` asks, as a text
    /// generator does (`TextGenerator::generate_until`): a request given
    /// up after it left fails as a lost answer, one never sent as
    /// `Unreachable`. A failure after part of the research was paid says
    /// what that part used.
    fn research_until(
        &self,
        key: &ApiKey,
        request: &ResearchRequest,
        stop: &Stop,
    ) -> Result<ResearchNotes, ProviderFailure>;

    /// Researches and waits for all of it.
    fn research(
        &self,
        key: &ApiKey,
        request: &ResearchRequest,
    ) -> Result<ResearchNotes, ProviderFailure> {
        self.research_until(key, request, &Stop::never())
    }

    /// The most a research within `limits` can use, to show before it
    /// starts: `sent` is the size in tokens of the instructions, the
    /// prompt and the pictures. How a provider bills what the model reads
    /// again at every step is its own, so each adapter answers for itself.
    fn ceiling(&self, limits: &ResearchLimits, sent: u64) -> Metered;
}

impl<T: WebResearcher + ?Sized> WebResearcher for Arc<T> {
    fn research_until(
        &self,
        key: &ApiKey,
        request: &ResearchRequest,
        stop: &Stop,
    ) -> Result<ResearchNotes, ProviderFailure> {
        (**self).research_until(key, request, stop)
    }

    fn ceiling(&self, limits: &ResearchLimits, sent: u64) -> Metered {
        (**self).ceiling(limits, sent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_product_research_is_limited_as_the_spec_says() {
        let limits = ResearchLimits::PRODUCT;
        assert_eq!(
            (limits.searches, limits.fetches, limits.page_tokens),
            (8, 5, 20_000)
        );
        // One step to begin, one after each search and fetch.
        assert_eq!(limits.steps(), 14);
    }

    #[test]
    fn the_ceiling_counts_every_page_and_search_at_its_limit() {
        let limits = ResearchLimits::PRODUCT;
        assert_eq!(limits.read_ceiling(), 5 * 20_000 + 8 * 6_000);
    }

    #[test]
    fn what_is_read_again_grows_with_every_result_largest_first() {
        let limits = ResearchLimits {
            searches: 1,
            fetches: 2,
            page_tokens: 10_000,
        };
        // After the first page 11,000 are held, after the second 21,000,
        // after the search 27,000: each is read once more.
        assert_eq!(limits.reread_ceiling(1_000), 11_000 + 21_000 + 27_000);
        assert_eq!(
            ResearchLimits {
                searches: 0,
                fetches: 0,
                page_tokens: 10_000
            }
            .reread_ceiling(1_000),
            0
        );
    }
}
