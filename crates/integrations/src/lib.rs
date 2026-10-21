//! Optional adapters for external tools.
//!
//! Each adapter reports its real availability and limits. None of them is
//! required for the core assessment flow.

pub mod burp;
pub mod firecrawl;
pub mod mcp;
pub mod nmap;

pub use burp::{BurpAdapter, BurpExchange, BurpStatus};
pub use firecrawl::{FirecrawlAdapter, FirecrawlPage};
pub use mcp::{McpConfig, McpConnection, McpTool};
pub use nmap::{NmapAdapter, NmapHost, NmapPort, NmapReport};

use darkbox_contracts::Capability;

pub fn capabilities() -> Vec<Capability> {
    vec![
        NmapAdapter::capability(),
        FirecrawlAdapter::capability(),
        BurpAdapter::capability(),
    ]
}
