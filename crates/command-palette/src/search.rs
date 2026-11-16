use crate::CommandPaletteConfig;
use crate::frecency::FrecencyStore;
use crate::providers::{PaletteResult, Provider, QueryContext};

pub struct SearchEngine {
    providers: Vec<Box<dyn Provider>>,
    frecency: FrecencyStore,
    config: CommandPaletteConfig,
}

impl SearchEngine {
    pub fn new(config: CommandPaletteConfig) -> Self {
        use crate::providers::*;

        let mut providers: Vec<Box<dyn Provider>> = Vec::new();

        if config.providers.apps {
            providers.push(Box::new(apps::AppsProvider::new()));
        }
        if config.providers.calculator {
            providers.push(Box::new(calculator::CalculatorProvider::new()));
        }
        if config.providers.web_search {
            providers.push(Box::new(web_search::WebSearchProvider::new(
                config.search_engine.clone(),
                config.custom_search_url.clone(),
            )));
        }
        if config.providers.shell_commands {
            providers.push(Box::new(shell::ShellProvider::new()));
        }
        if config.providers.system_commands {
            providers.push(Box::new(system::SystemProvider::new()));
        }
        if config.providers.file_search {
            providers.push(Box::new(files::FilesProvider::new(
                config.file_search_tool.clone(),
            )));
        }
        if config.providers.settings {
            providers.push(Box::new(settings::SettingsProvider::new()));
        }
        if config.providers.window_walker {
            providers.push(Box::new(windows::WindowsProvider::new(
                config.keywords.window_walker.clone(),
                config.global_results.window_walker,
            )));
        }
        if config.providers.clipboard_history {
            providers.push(Box::new(clipboard::ClipboardProvider::new(
                config.keywords.clipboard_history.clone(),
            )));
        }
        if config.providers.bookmarks {
            providers.push(Box::new(bookmarks::BookmarksProvider::new(
                config.keywords.bookmarks.clone(),
                config.global_results.bookmarks,
                config.bookmarks.clone(),
            )));
        }
        if config.providers.time_date {
            providers.push(Box::new(time_date::TimeDateProvider::new(
                config.keywords.time_date.clone(),
                config.global_results.time_date,
            )));
        }
        if config.providers.packages {
            providers.push(Box::new(packages::PackagesProvider::new(
                config.keywords.packages.clone(),
                config.package_search.clone(),
            )));
        }

        let frecency = FrecencyStore::load();

        Self {
            providers,
            frecency,
            config,
        }
    }

    pub fn initialize(&mut self) {
        for provider in &mut self.providers {
            if let Err(e) = provider.initialize() {
                tracing::warn!("Failed to initialize provider '{}': {e}", provider.tag());
            }
        }
    }

    pub fn search(&self, raw_query: &str) -> Vec<PaletteResult> {
        let raw_query = raw_query.trim();
        if raw_query.is_empty() {
            return Vec::new();
        }

        let mut all_results = Vec::new();

        // A query starting with a provider keyword (e.g. `<`, `clip `) goes
        // only to the provider(s) owning that keyword.
        let claimed = self.providers.iter().any(|p| p.keyword_match(raw_query));

        for provider in &self.providers {
            if claimed && !provider.keyword_match(raw_query) {
                continue;
            }
            if !provider.matches(raw_query) {
                continue;
            }

            let stripped = provider.strip_prefix(raw_query);
            let ctx = QueryContext {
                raw_query,
                stripped_query: stripped,
                max_results: self.config.max_results,
            };

            let mut results = provider.search(&ctx);

            // Apply frecency boost
            for result in &mut results {
                let key = format!("{}:{}", result.provider_tag, result.id);
                result.score += self.frecency.score(&key);
            }

            all_results.extend(results);
        }

        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_results.truncate(self.config.max_results);

        all_results
    }

    /// True when a background provider (package search) has new results
    /// and the current query should be searched again.
    pub fn has_async_updates(&self) -> bool {
        // Poll all providers so every flag is cleared.
        self.providers
            .iter()
            .fold(false, |any, p| p.poll_updates() || any)
    }

    pub fn record_activation(&mut self, result: &PaletteResult) {
        let key = format!("{}:{}", result.provider_tag, result.id);
        self.frecency.record_launch(&key);
        self.frecency.save();
    }
}
