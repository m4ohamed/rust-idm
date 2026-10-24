//! الزاحف المحدود لسحب المواقع (Bounded BFS Site Crawler)

use anyhow::Result;
use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashSet, VecDeque},
    time::Duration,
};
use tokio::time::{sleep, timeout};
use url::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlConfig {
    pub start_url: String,
    pub max_depth: usize,
    pub max_pages: usize,
    pub max_concurrent_requests: usize,
    pub request_timeout_ms: u64,
    pub max_page_size_bytes: usize,
    pub delay_between_requests_ms: u64,
    pub same_domain_only: bool,
    pub allowed_extensions: Vec<String>,
    pub excluded_paths: Vec<String>,
}

impl Default for CrawlConfig {
    fn default() -> Self {
        Self {
            start_url: String::new(),
            max_depth: 2,
            max_pages: 25,
            max_concurrent_requests: 4,
            request_timeout_ms: 10_000,
            max_page_size_bytes: 5 * 1024 * 1024,
            delay_between_requests_ms: 100,
            same_domain_only: true,
            allowed_extensions: vec!["mp4".into(), "pdf".into(), "zip".into()],
            excluded_paths: vec!["/logout".into(), "/admin".into()],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrawlReport {
    pub pages_visited: usize,
    pub pages_discovered: usize,
    pub matched_files: Vec<String>,
    pub errors: Vec<String>,
}

pub struct BoundedSiteCrawler {
    client: Client,
}

impl BoundedSiteCrawler {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub fn normalize_url(raw: &Url) -> String {
        let mut clean = raw.clone();
        clean.set_fragment(None);
        clean.to_string()
    }

    pub async fn crawl(&self, cfg: &CrawlConfig) -> Result<CrawlReport> {
        let start_parsed = Url::parse(&cfg.start_url)?;
        let start_domain = start_parsed.host_str().unwrap_or("").to_lowercase();
        let link_re = Regex::new(r#"(?i)(?:href|src)\s*=\s*["']([^"'#]+)["']"#)?;

        let mut visited_pages: HashSet<String> = HashSet::new();
        let mut matched_set: HashSet<String> = HashSet::new();
        let mut matched_files: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        let mut queue: VecDeque<(Url, usize)> = VecDeque::new();

        queue.push_back((start_parsed.clone(), 0));
        visited_pages.insert(Self::normalize_url(&start_parsed));

        while let Some((current_url, depth)) = queue.pop_front() {
            if visited_pages.len() > cfg.max_pages && depth > 0 {
                break;
            }

            if cfg.delay_between_requests_ms > 0 && !errors.is_empty() {
                sleep(Duration::from_millis(cfg.delay_between_requests_ms)).await;
            }

            let resp_res = timeout(
                Duration::from_millis(cfg.request_timeout_ms),
                self.client.get(current_url.as_str()).send(),
            )
            .await;

            let resp = match resp_res {
                Ok(Ok(r)) if r.status().is_success() => r,
                Ok(Ok(r)) => {
                    errors.push(format!("{} -> HTTP {}", current_url, r.status()));
                    continue;
                }
                Ok(Err(e)) => {
                    errors.push(format!("{} -> {}", current_url, e));
                    continue;
                }
                Err(_) => {
                    errors.push(format!("{} -> Timeout", current_url));
                    continue;
                }
            };

            let bytes = match resp.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    errors.push(format!("{} -> {}", current_url, e));
                    continue;
                }
            };

            let slice = if bytes.len() > cfg.max_page_size_bytes {
                &bytes[..cfg.max_page_size_bytes]
            } else {
                &bytes[..]
            };
            let html = String::from_utf8_lossy(slice);

            for cap in link_re.captures_iter(&html) {
                if let Some(raw_attr) = cap.get(1) {
                    if let Ok(resolved) = current_url.join(raw_attr.as_str()) {
                        if resolved.scheme() != "http" && resolved.scheme() != "https" {
                            continue;
                        }
                        let norm = Self::normalize_url(&resolved);
                        let path_lower = resolved.path().to_lowercase();

                        if cfg
                            .excluded_paths
                            .iter()
                            .any(|ex| !ex.is_empty() && path_lower.starts_with(&ex.to_lowercase()))
                        {
                            continue;
                        }

                        if cfg.same_domain_only {
                            let host = resolved.host_str().unwrap_or("").to_lowercase();
                            if host != start_domain {
                                continue;
                            }
                        }

                        let is_target_file = cfg.allowed_extensions.is_empty()
                            || cfg.allowed_extensions.iter().any(|ext| {
                                path_lower.ends_with(&format!(".{}", ext.to_lowercase()))
                            });

                        if is_target_file {
                            if matched_set.insert(norm.clone()) {
                                matched_files.push(norm);
                            }
                        } else if depth < cfg.max_depth && visited_pages.len() < cfg.max_pages {
                            if visited_pages.insert(norm) {
                                queue.push_back((resolved, depth + 1));
                            }
                        }
                    }
                }
            }
        }

        Ok(CrawlReport {
            pages_visited: visited_pages.len(),
            pages_discovered: visited_pages.len() + queue.len(),
            matched_files,
            errors,
        })
    }
}
