//! Generic table rendering for metric snapshots.

use heimwatch_core::MetricRecord;

/// Configuration for rendering a metric table.
pub struct TableConfig {
    /// Table title (e.g., "Heimwatch CPU Usage Snapshot")
    pub title: String,
    /// Column headers and their display width
    pub columns: Vec<(&'static str, usize)>,
    /// Total table width for separator lines
    pub total_width: usize,
    /// Header line width for separator
    pub header_width: usize,
}

/// Render a generic table with the given records and config.
///
/// Handles: title, headers, separators, empty-record message, app name truncation.
/// Caller provides sorting/aggregation logic via the filtered records vector.
pub fn print_table(
    config: &TableConfig,
    records: &[MetricRecord],
    format_rows: impl Fn(&[MetricRecord]) -> Vec<Vec<String>>,
    format_total: impl Fn() -> Vec<String>,
) {
    println!("\n{}", config.title);
    println!("{}", "─".repeat(config.total_width));

    // Print headers
    let header_parts: Vec<&str> = config.columns.iter().map(|(name, _)| *name).collect();
    println!("  {}", header_parts.join("  "));
    println!("  {}", "─".repeat(config.header_width));

    if records.is_empty() {
        println!("  (no data)");
    } else {
        // Format and print data rows
        let rows = format_rows(records);
        for row in rows {
            println!("  {}", row.join("  "));
        }

        println!("{}", "─".repeat(config.total_width));

        // Print totals row
        let total = format_total();
        println!("  {}", total.join("  "));
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_table_config() {
        let config = TableConfig {
            title: "Test Table (5s window)".to_string(),
            columns: vec![("App", 28), ("Value", 10)],
            total_width: 50,
            header_width: 40,
        };
        assert_eq!(config.title, "Test Table (5s window)");
        assert_eq!(config.columns.len(), 2);
    }
}
