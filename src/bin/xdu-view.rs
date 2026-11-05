#![allow(clippy::collapsible_if)]
#![allow(clippy::collapsible_else_if)]

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, stdout};
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::{
    ExecutableCommand,
    event::{self, Event, KeyCode, KeyEventKind},
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use duckdb::Connection;
use ratatui::{
    prelude::*,
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
};

use xdu::{
    QueryFilters, ROOT_PARTITION, SortMode, format_bytes, glob_to_regex, index_completion_warning,
    index_glob, index_version_error, parse_size,
};

/// Detect file type from magic bytes, shebangs, text content, and extension.
///
/// Returns `(description, is_text)` where `description` is a human-readable
/// string like "ELF 64-bit LSB executable, x86-64" or "Python script, UTF-8 text"
/// and `is_text` indicates whether the file should be treated as scrollable text.
fn detect_file_type(sniff_buf: &[u8], file_path: &str) -> (String, bool) {
    // --- Layer 1: infer magic bytes (with enrichment) ---
    if let Some(kind) = infer::get(sniff_buf) {
        let mime = kind.mime_type();
        // ELF enrichment
        if mime == "application/x-executable"
            || mime == "application/x-sharedlib"
            || (sniff_buf.len() >= 4 && &sniff_buf[..4] == b"\x7FELF")
        {
            if let Some(desc) = describe_elf(sniff_buf) {
                return (desc, false);
            }
        }
        // For text/* MIME types (e.g. text/x-shellscript), fall through to
        // our shebang and text sub-type layers for richer descriptions and
        // correct is_text=true handling.
        if !mime.starts_with("text/") {
            return (format!("{} ({})", mime, kind.extension()), false);
        }
    }

    // ELF that infer missed (it should catch it, but just in case)
    if sniff_buf.len() >= 4 && &sniff_buf[..4] == b"\x7FELF" {
        if let Some(desc) = describe_elf(sniff_buf) {
            return (desc, false);
        }
        return ("ELF binary".to_string(), false);
    }

    // Mach-O enrichment (infer may not detect all variants)
    if sniff_buf.len() >= 8 {
        let magic = &sniff_buf[..4];
        if magic == b"\xCF\xFA\xED\xFE"
            || magic == b"\xFE\xED\xFA\xCF"
            || magic == b"\xCE\xFA\xED\xFE"
            || magic == b"\xFE\xED\xFA\xCE"
        {
            if let Some(desc) = describe_macho(sniff_buf) {
                return (desc, false);
            }
            return ("Mach-O binary".to_string(), false);
        }
        // Universal (fat) binary
        if magic == b"\xCA\xFE\xBA\xBE" || magic == b"\xBE\xBA\xFE\xCA" {
            return ("Mach-O universal binary".to_string(), false);
        }
    }

    // --- Layer 2: Shebang detection ---
    if sniff_buf.len() >= 2 && &sniff_buf[..2] == b"#!" {
        if let Some(desc) = describe_shebang(sniff_buf) {
            return (desc, true);
        }
    }

    // --- Layer 3: Text sub-type sniffing ---
    if let Ok(text) = std::str::from_utf8(sniff_buf) {
        // Content-based detection
        let trimmed = text.trim_start();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return ("JSON text, UTF-8".to_string(), true);
        }
        if trimmed.starts_with("<?xml") || trimmed.starts_with("<!DOCTYPE") {
            return ("XML text, UTF-8".to_string(), true);
        }

        // Extension-based fallback for text files
        if let Some(desc) = describe_text_by_extension(file_path) {
            return (format!("{}, UTF-8", desc), true);
        }

        // --- Layer 4a: Generic UTF-8 text ---
        return ("text/plain; UTF-8".to_string(), true);
    }

    // --- Layer 4b: Binary fallback ---
    ("application/octet-stream (binary data)".to_string(), false)
}

/// Parse ELF header fields from the sniff buffer.
fn describe_elf(buf: &[u8]) -> Option<String> {
    if buf.len() < 20 {
        return None;
    }
    let class = match buf[4] {
        1 => "32-bit",
        2 => "64-bit",
        _ => "unknown-class",
    };
    let endian = match buf[5] {
        1 => "LSB",
        2 => "MSB",
        _ => "unknown-endian",
    };
    let e_type_val = if buf[5] == 1 {
        u16::from_le_bytes([buf[16], buf[17]])
    } else {
        u16::from_be_bytes([buf[16], buf[17]])
    };
    let etype = match e_type_val {
        1 => "relocatable",
        2 => "executable",
        3 => "shared object",
        4 => "core dump",
        _ => "object",
    };
    let e_machine_val = if buf[5] == 1 {
        u16::from_le_bytes([buf[18], buf[19]])
    } else {
        u16::from_be_bytes([buf[18], buf[19]])
    };
    let arch = match e_machine_val {
        0x03 => "x86",
        0x08 => "MIPS",
        0x14 => "PowerPC",
        0x15 => "PowerPC64",
        0x28 => "ARM",
        0x2A => "SuperH",
        0x32 => "IA-64",
        0x3E => "x86-64",
        0xB7 => "AArch64",
        0xF3 => "RISC-V",
        0xF7 => "BPF",
        _ => "unknown-arch",
    };
    Some(format!("ELF {} {} {}, {}", class, endian, etype, arch))
}

/// Parse Mach-O header fields from the sniff buffer.
fn describe_macho(buf: &[u8]) -> Option<String> {
    if buf.len() < 8 {
        return None;
    }
    let magic = &buf[..4];
    let (class, le) = match magic {
        b"\xCF\xFA\xED\xFE" => ("64-bit", true),  // MH_MAGIC_64 (LE)
        b"\xFE\xED\xFA\xCF" => ("64-bit", false), // MH_MAGIC_64 (BE)
        b"\xCE\xFA\xED\xFE" => ("32-bit", true),  // MH_MAGIC (LE)
        b"\xFE\xED\xFA\xCE" => ("32-bit", false), // MH_MAGIC (BE)
        _ => return None,
    };
    let cpu_type = if le {
        u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]])
    } else {
        u32::from_be_bytes([buf[4], buf[5], buf[6], buf[7]])
    };
    let arch = match cpu_type {
        7 => "x86",                 // CPU_TYPE_X86
        0x0100_0007 => "x86-64",    // CPU_TYPE_X86_64
        12 => "ARM",                // CPU_TYPE_ARM
        0x0100_000C => "ARM64",     // CPU_TYPE_ARM64
        18 => "PowerPC",            // CPU_TYPE_POWERPC
        0x0100_0012 => "PowerPC64", // CPU_TYPE_POWERPC64
        _ => "unknown-arch",
    };
    // file_type at offset 12 (4 bytes)
    let ftype = if buf.len() >= 16 {
        let ft = if le {
            u32::from_le_bytes([buf[12], buf[13], buf[14], buf[15]])
        } else {
            u32::from_be_bytes([buf[12], buf[13], buf[14], buf[15]])
        };
        match ft {
            1 => "object",
            2 => "executable",
            3 => "fixed VM shared library",
            4 => "core dump",
            5 => "preloaded executable",
            6 => "dylib",
            7 => "dynamic linker",
            8 => "bundle",
            _ => "binary",
        }
    } else {
        "binary"
    };
    Some(format!("Mach-O {} {}, {}", class, ftype, arch))
}

/// Parse a shebang line to identify the script type.
fn describe_shebang(buf: &[u8]) -> Option<String> {
    // Find the end of the first line
    let end = buf
        .iter()
        .position(|&b| b == b'\n')
        .unwrap_or(buf.len().min(256));
    let line = std::str::from_utf8(&buf[2..end]).ok()?.trim();
    if line.is_empty() {
        return None;
    }

    // Handle "env" indirection: "#!/usr/bin/env python3" -> "python3"
    let interpreter = if line.contains("env ") || line.contains("env\t") {
        line.split_whitespace().last()?
    } else {
        // "/usr/bin/perl" -> "perl", "/bin/bash" -> "bash"
        line.split_whitespace().next()?.rsplit('/').next()?
    };

    let desc = match interpreter {
        "bash" => "Bash script",
        "sh" => "POSIX shell script",
        "zsh" => "Zsh script",
        "fish" => "Fish script",
        "dash" => "Dash script",
        i if i.starts_with("python") => "Python script",
        "perl" => "Perl script",
        "ruby" => "Ruby script",
        "node" | "nodejs" => "Node.js script",
        "Rscript" => "R script",
        "lua" => "Lua script",
        "php" => "PHP script",
        "awk" | "gawk" | "mawk" => "AWK script",
        "sed" => "Sed script",
        "tclsh" | "wish" => "Tcl script",
        _ => return Some(format!("Script (#!{}), UTF-8 text", interpreter)),
    };
    Some(format!("{}, UTF-8 text", desc))
}

/// Map file extension to a human-readable text type description.
fn describe_text_by_extension(path: &str) -> Option<&'static str> {
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    let desc = match ext.as_str() {
        "csv" => "CSV text",
        "tsv" => "TSV text",
        "json" => "JSON text",
        "jsonl" | "ndjson" => "JSON Lines text",
        "yaml" | "yml" => "YAML text",
        "toml" => "TOML text",
        "xml" => "XML text",
        "html" | "htm" => "HTML text",
        "css" => "CSS text",
        "md" | "markdown" => "Markdown text",
        "rst" => "reStructuredText",
        "tex" | "latex" => "LaTeX text",
        "py" => "Python source",
        "rs" => "Rust source",
        "c" => "C source",
        "h" => "C header",
        "cpp" | "cc" | "cxx" => "C++ source",
        "hpp" | "hh" | "hxx" => "C++ header",
        "java" => "Java source",
        "go" => "Go source",
        "js" | "mjs" | "cjs" => "JavaScript source",
        "ts" | "mts" | "cts" => "TypeScript source",
        "rb" => "Ruby source",
        "pl" | "pm" => "Perl source",
        "php" => "PHP source",
        "r" => "R source",
        "jl" => "Julia source",
        "lua" => "Lua source",
        "swift" => "Swift source",
        "kt" | "kts" => "Kotlin source",
        "scala" => "Scala source",
        "hs" => "Haskell source",
        "ml" | "mli" => "OCaml source",
        "ex" | "exs" => "Elixir source",
        "erl" | "hrl" => "Erlang source",
        "f" | "f90" | "f95" | "f03" | "f08" | "for" => "Fortran source",
        "sh" | "bash" => "Shell script",
        "zsh" => "Zsh script",
        "fish" => "Fish script",
        "ps1" => "PowerShell script",
        "sql" => "SQL text",
        "graphql" | "gql" => "GraphQL text",
        "proto" => "Protocol Buffers text",
        "ini" | "cfg" | "conf" => "Configuration text",
        "env" => "Environment file",
        "txt" | "text" | "log" => "Plain text",
        "makefile" => "Makefile",
        _ => return None,
    };
    Some(desc)
}

/// Format file count with K/M/B suffixes
fn format_file_count(count: i64) -> String {
    if count >= 1_000_000_000 {
        format!("{:.1}B files", count as f64 / 1_000_000_000.0)
    } else if count >= 1_000_000 {
        format!("{:.1}M files", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K files", count as f64 / 1_000.0)
    } else if count == 1 {
        "1 file".to_string()
    } else {
        format!("{} files", count)
    }
}

use xdu::cli::XduViewArgs;

/// Represents a directory entry in the view
#[derive(Clone, Debug)]
struct DirEntry {
    name: String,
    path: String,
    is_dir: bool,
    total_size: i64,
    file_count: i64,
    latest_atime: i64,
}

/// Overlay target for list-mode Space. Directories are excluded so Space on a
/// dir is a no-op rather than a preview of something that has no text; Enter
/// and → still drill in.
fn list_preview_entry(entries: &[DirEntry], selected: Option<usize>) -> Option<&DirEntry> {
    let idx = selected?;
    let entry = entries.get(idx)?;
    if entry.is_dir { None } else { Some(entry) }
}

/// Input mode for interactive filter entry
#[derive(Clone, Debug, PartialEq)]
enum InputMode {
    /// Normal navigation mode
    Normal,
    /// Entering a pattern filter
    Pattern,
    /// Entering older-than days
    OlderThan,
    /// Entering newer-than days
    NewerThan,
    /// Entering min-size
    MinSize,
    /// Entering max-size
    MaxSize,
    /// Selecting sort mode
    SortSelect,
    /// List-mode file preview overlay. Distinct from SortSelect so Esc
    /// dismisses the overlay instead of quitting.
    ListPreview,
}

impl InputMode {
    fn prompt(&self) -> &'static str {
        match self {
            InputMode::Normal => "",
            InputMode::Pattern => "Pattern (glob): ",
            InputMode::OlderThan => "Older than (days): ",
            InputMode::NewerThan => "Newer than (days): ",
            InputMode::MinSize => "Min size (e.g., 1M): ",
            InputMode::MaxSize => "Max size (e.g., 1G): ",
            InputMode::SortSelect | InputMode::ListPreview => "",
        }
    }
}

/// View mode for the TUI
#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewMode {
    /// Traditional single-list view (default)
    List,
    /// Miller columns (tree) view — horizontal cascade of directory hierarchy
    Tree,
}

/// Strip ANSI escape sequences (CSI, OSC, and single-byte ESC sequences) from a string.
/// Raw escapes corrupt ratatui's cell-width accounting and can bleed into other panes.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1B' {
            match chars.peek() {
                // CSI sequence: ESC [ ... final_byte
                Some('[') => {
                    chars.next();
                    // Consume parameter bytes (0x30-0x3F), intermediate bytes (0x20-0x2F),
                    // then the final byte (0x40-0x7E)
                    while let Some(&ch) = chars.peek() {
                        if ('\x40'..='\x7E').contains(&ch) {
                            chars.next(); // consume final byte
                            break;
                        }
                        chars.next();
                    }
                }
                // OSC sequence: ESC ] ... ST (ST = ESC \ or BEL)
                Some(']') => {
                    chars.next();
                    while let Some(ch) = chars.next() {
                        if ch == '\x07' {
                            break;
                        } // BEL terminator
                        if ch == '\x1B' {
                            if chars.peek() == Some(&'\\') {
                                chars.next(); // consume \
                            }
                            break;
                        }
                    }
                }
                // Two-byte ESC sequence: ESC + one character
                Some(_) => {
                    chars.next();
                }
                // Bare ESC at end of string
                None => {}
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Maximum lines to keep in the sliding window buffer.
const MAX_BUFFER_LINES: usize = 100_000;
/// Chunk size for streaming reads (64KB).
const CHUNK_BYTES: usize = 65_536;

/// Cached file preview for the rightmost pane in tree view.
struct FilePreview {
    /// Full file path on disk
    path: String,
    /// File size from the index
    size: i64,
    /// Access time from the index
    atime: i64,
    /// Human-readable type description (e.g. "PNG image", "text/plain; UTF-8")
    type_description: String,
    /// Whether the file is detected as plain text
    is_text: bool,
    /// Content lines (sliding window for text files, empty for binary)
    lines: VecDeque<String>,
    /// Global line number of `lines[0]`
    first_line_number: usize,
    /// Total lines read from the file so far
    total_lines_loaded: usize,
    /// Byte position where the last read stopped
    file_offset: u64,
    /// Whether we've hit EOF
    eof_reached: bool,
    /// Current scroll offset — a global line number
    scroll_offset: usize,
}

/// A single column in the Miller columns (tree) view.
struct Column {
    /// Display title for the column header
    title: String,
    /// Partition name (None = partition list column)
    partition: Option<String>,
    /// Root path for the partition (empty for partition list)
    partition_root: String,
    /// Directory path this column represents
    path: String,
    /// Entries in this column
    entries: Vec<DirEntry>,
    /// Selection state
    list_state: ListState,
}

/// Application state
struct App {
    conn: Connection,
    index_path: PathBuf,

    /// Current absolute path prefix (the root path for the current partition)
    partition_root: String,

    /// Current directory path (absolute, empty = partition list)
    current_path: String,

    /// Current partition being viewed (None = viewing partition list)
    current_partition: Option<String>,

    /// Entries in the current view
    entries: Vec<DirEntry>,

    /// List selection state
    list_state: ListState,

    /// Whether we're currently loading
    loading: bool,

    /// Status message
    status: String,

    /// Query filters
    filters: QueryFilters,

    /// Sort mode
    sort_mode: SortMode,

    /// Current input mode
    input_mode: InputMode,

    /// Current input buffer
    input_buffer: String,

    /// Pending sort mode (for sort selection)
    pending_sort: SortMode,

    /// Current view mode (list or tree)
    view_mode: ViewMode,

    /// Columns for Miller columns (tree) view
    columns: Vec<Column>,

    /// Active column index in tree view
    active_column: usize,

    /// File preview for the rightmost pane in tree view
    file_preview: Option<FilePreview>,

    /// Whether the preview pane pager is focused (less-like scroll mode)
    preview_focused: bool,

    /// List-mode overlay. Separate from `file_preview` so tree state cannot leak
    /// into the overlay (and vice versa) across a view-mode toggle.
    list_preview: Option<FilePreview>,
}

impl App {
    fn new(
        conn: Connection,
        index_path: PathBuf,
        initial_partition: Option<String>,
        filters: QueryFilters,
        sort_mode: SortMode,
    ) -> Result<Self> {
        let mut app = App {
            conn,
            index_path,
            partition_root: String::new(),
            current_path: String::new(),
            current_partition: None,
            entries: Vec::new(),
            list_state: ListState::default(),
            loading: false,
            status: String::new(),
            filters,
            sort_mode,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            pending_sort: sort_mode,
            view_mode: ViewMode::List,
            columns: Vec::new(),
            active_column: 0,
            file_preview: None,
            preview_focused: false,
            list_preview: None,
        };

        if let Some(partition) = initial_partition {
            app.current_partition = Some(partition);
            app.load_directory()?;
        } else {
            app.load_partitions()?;
        }

        if !app.entries.is_empty() {
            app.list_state.select(Some(0));
        }

        Ok(app)
    }

    /// Query individual files from the __root__ partition.
    /// Returns DirEntry items with is_dir=false and file_count=1.
    fn query_root_files(&self) -> Result<Vec<DirEntry>> {
        let glob = index_glob(&self.index_path, Some(ROOT_PARTITION));
        let where_clause = self.filters.to_full_where_clause();

        let sql = format!(
            r#"
            SELECT path, size, atime
            FROM read_parquet('{glob}')
            {where_clause}
            "#,
            glob = glob,
            where_clause = where_clause
        );

        let mut entries = Vec::new();
        let mut stmt = match self.conn.prepare(&sql) {
            Ok(s) => s,
            Err(_) => return Ok(entries), // No __root__ partition exists
        };
        let mut rows = match stmt.query([]) {
            Ok(r) => r,
            Err(_) => return Ok(entries),
        };

        while let Some(row) = rows.next()? {
            let path: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let atime: i64 = row.get(2)?;
            let name = path.rsplit('/').next().unwrap_or(&path).to_string();
            entries.push(DirEntry {
                name,
                path,
                is_dir: false,
                total_size: size,
                file_count: 1,
                latest_atime: atime,
            });
        }

        Ok(entries)
    }

    /// Sort entries in-place according to the current SortMode.
    fn sort_entries(entries: &mut [DirEntry], sort_mode: SortMode) {
        match sort_mode {
            SortMode::Name => {
                // Directories first, then alphabetical by name
                entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
            }
            SortMode::SizeDesc => entries.sort_by_key(|a| std::cmp::Reverse(a.total_size)),
            SortMode::SizeAsc => entries.sort_by_key(|a| a.total_size),
            SortMode::CountDesc => entries.sort_by_key(|a| std::cmp::Reverse(a.file_count)),
            SortMode::CountAsc => entries.sort_by_key(|a| a.file_count),
            SortMode::AgeDesc => entries.sort_by_key(|a| a.latest_atime), // oldest first
            SortMode::AgeAsc => entries.sort_by_key(|a| std::cmp::Reverse(a.latest_atime)), // newest first
        }
    }

    /// Load the list of partitions
    fn load_partitions(&mut self) -> Result<()> {
        self.loading = true;
        let start = Instant::now();

        // The partition name is the directory holding the chunk, recovered below from the
        // `filename` pseudo-column. Not Hive partitioning: the layout uses bare directory names,
        // so `hive_partitioning=true` would yield no partition column at all.
        let glob = index_glob(&self.index_path, None);

        // Build filter clause — exclude __root__ (its files are merged as individual entries)
        let filter_clause = self.filters.to_where_clause();
        let having_clause = if filter_clause.is_empty() {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}'",
                ROOT_PARTITION
            )
        } else {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}' AND SUM(CASE WHEN {} THEN 1 ELSE 0 END) > 0",
                ROOT_PARTITION, filter_clause
            )
        };

        // Query using filename to extract partition from the parquet file path
        // The partition name is the directory containing the parquet file
        // When filters are active, only count/sum matching files
        let sql = if self.filters.is_active() {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(CASE WHEN {filter} THEN size ELSE 0 END) as total_size,
                    MAX(CASE WHEN {filter} THEN atime ELSE 0 END) as latest_atime,
                    SUM(CASE WHEN {filter} THEN 1 ELSE 0 END) as file_count
                FROM read_parquet('{glob}', filename=true)
                GROUP BY partition
                {having}
                ORDER BY {order}
                "#,
                filter = filter_clause,
                glob = glob,
                having = having_clause,
                order = self.sort_mode.to_partition_order_by()
            )
        } else {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(size) as total_size,
                    MAX(atime) as latest_atime,
                    COUNT(*) as file_count
                FROM read_parquet('{glob}', filename=true)
                GROUP BY partition
                HAVING partition IS NOT NULL AND partition != '' AND partition != '{root}'
                ORDER BY {order}
                "#,
                glob = glob,
                root = ROOT_PARTITION,
                order = self.sort_mode.to_partition_order_by()
            )
        };

        self.entries.clear();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let total_size: i64 = row.get(1)?;
            let latest_atime: i64 = row.get(2)?;
            let file_count: i64 = row.get(3)?;

            self.entries.push(DirEntry {
                name: name.clone(),
                path: name,
                is_dir: true,
                total_size,
                file_count,
                latest_atime,
            });
        }

        // Merge individual files from the __root__ partition
        let root_files = self.query_root_files()?;
        if !root_files.is_empty() {
            self.entries.extend(root_files);
            Self::sort_entries(&mut self.entries, self.sort_mode);
        }

        self.status = format!(
            "{} entries loaded in {:.2}s",
            self.entries.len(),
            start.elapsed().as_secs_f64()
        );
        self.loading = false;
        Ok(())
    }

    /// Discover the common root path for a partition
    fn discover_partition_root(&self, partition: &str) -> Result<String> {
        let glob = index_glob(&self.index_path, Some(partition));

        // Get the shortest path to find the common root
        let sql = format!(
            "SELECT path FROM read_parquet('{}') ORDER BY length(path) LIMIT 1",
            glob
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let sample_path: String = row.get(0)?;
            // Find the directory containing this file
            if let Some(pos) = sample_path.rfind('/') {
                return Ok(sample_path[..pos].to_string());
            }
        }

        Ok(String::new())
    }

    /// Load directory contents for the current path
    fn load_directory(&mut self) -> Result<()> {
        self.loading = true;
        let start = Instant::now();

        let partition = self.current_partition.as_ref().unwrap();
        let glob = index_glob(&self.index_path, Some(partition));

        // If we don't have a partition root yet, discover it
        if self.partition_root.is_empty() {
            self.partition_root = self.discover_partition_root(partition)?;
            self.current_path = self.partition_root.clone();
        }

        // Build the path prefix we're looking at (with trailing slash for LIKE)
        let prefix = format!("{}/", self.current_path);

        // Query to get entries at this level
        // We extract the next path component after the current path prefix
        let prefix_len = prefix.len();

        // Build filter conditions
        let filter_clause = self.filters.to_where_clause();
        let file_filter = if filter_clause.is_empty() {
            format!("path LIKE '{}%'", prefix)
        } else {
            format!("path LIKE '{}%' AND {}", prefix, filter_clause)
        };

        let order_by = self.sort_mode.to_order_by(self.sort_mode == SortMode::Name);

        let sql = format!(
            r#"
            WITH files AS (
                SELECT 
                    path,
                    size,
                    atime
                FROM read_parquet('{glob}')
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path,
                    size,
                    atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
            glob = glob,
            file_filter = file_filter,
            prefix_len = prefix_len,
            order_by = order_by
        );

        self.entries.clear();

        // Add parent entry (always show ".." to go back)
        self.entries.push(DirEntry {
            name: "..".to_string(),
            path: "..".to_string(),
            is_dir: true,
            total_size: 0,
            file_count: 0,
            latest_atime: 0,
        });

        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let component: String = row.get(0)?;
            let is_dir: bool = row.get(1)?;
            let total_size: i64 = row.get(2)?;
            let file_count: i64 = row.get(3)?;
            let latest_atime: i64 = row.get(4)?;

            self.entries.push(DirEntry {
                name: component.clone(),
                path: format!("{}/{}", self.current_path, component),
                is_dir,
                total_size,
                file_count,
                latest_atime,
            });
        }

        let elapsed = start.elapsed().as_secs_f64();
        let filter_info = if self.filters.is_active() {
            " (filtered)"
        } else {
            ""
        };
        self.status = format!(
            "{} entries in {:.2}s{}",
            self.entries.len(),
            elapsed,
            filter_info
        );
        self.loading = false;
        Ok(())
    }

    /// Start sort selection mode
    fn start_sort_select(&mut self) {
        self.pending_sort = self.sort_mode;
        self.input_mode = InputMode::SortSelect;
    }

    /// Cycle pending sort to next mode
    fn sort_select_next(&mut self) {
        self.pending_sort = self.pending_sort.next();
    }

    /// Cycle pending sort to previous mode
    fn sort_select_prev(&mut self) {
        self.pending_sort = self.pending_sort.prev();
    }

    /// Confirm sort selection and reload
    fn confirm_sort(&mut self) -> Result<()> {
        self.sort_mode = self.pending_sort;
        self.input_mode = InputMode::Normal;
        self.reload()
    }

    /// Cancel sort selection
    fn cancel_sort(&mut self) {
        self.pending_sort = self.sort_mode;
        self.input_mode = InputMode::Normal;
    }

    /// Reload the current view
    fn reload(&mut self) -> Result<()> {
        match self.view_mode {
            ViewMode::List => {
                if self.current_partition.is_none() {
                    self.load_partitions()?;
                } else {
                    self.load_directory()?;
                }
                // Preserve selection if possible
                if let Some(idx) = self.list_state.selected() {
                    if idx >= self.entries.len() && !self.entries.is_empty() {
                        self.list_state.select(Some(self.entries.len() - 1));
                    }
                }
            }
            ViewMode::Tree => {
                self.init_tree()?;
            }
        }
        Ok(())
    }

    /// Start input mode for a filter
    fn start_input(&mut self, mode: InputMode) {
        self.input_mode = mode;
        self.input_buffer.clear();
    }

    /// Cancel input mode
    fn cancel_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
    }

    /// Confirm input and apply filter
    fn confirm_input(&mut self) -> Result<()> {
        let value = self.input_buffer.trim().to_string();

        match self.input_mode {
            InputMode::Pattern => {
                if value.is_empty() {
                    self.filters.pattern = None;
                    self.filters.pattern_display = None;
                } else {
                    // The prompt speaks the same glob dialect as the flag; the stored
                    // field stays regex whichever entry point set it.
                    match glob_to_regex(&value) {
                        Ok(translated) => {
                            self.filters.pattern = Some(translated);
                            self.filters.pattern_display = Some(value);
                        }
                        Err(e) => {
                            self.status = format!("Invalid pattern: {}", e);
                            self.input_mode = InputMode::Normal;
                            self.input_buffer.clear();
                            return Ok(());
                        }
                    }
                }
            }
            InputMode::OlderThan => {
                if value.is_empty() {
                    self.filters.older_than = None;
                } else if let Ok(days) = value.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    self.filters.older_than = Some(now - (days as i64 * 86400));
                } else {
                    self.status = format!("Invalid number: {}", value);
                    self.input_mode = InputMode::Normal;
                    self.input_buffer.clear();
                    return Ok(());
                }
            }
            InputMode::NewerThan => {
                if value.is_empty() {
                    self.filters.newer_than = None;
                } else if let Ok(days) = value.parse::<u64>() {
                    let now = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() as i64;
                    self.filters.newer_than = Some(now - (days as i64 * 86400));
                } else {
                    self.status = format!("Invalid number: {}", value);
                    self.input_mode = InputMode::Normal;
                    self.input_buffer.clear();
                    return Ok(());
                }
            }
            InputMode::MinSize => {
                if value.is_empty() {
                    self.filters.min_size = None;
                } else {
                    match parse_size(&value) {
                        Ok(size) => self.filters.min_size = Some(size),
                        Err(e) => {
                            self.status = e;
                            self.input_mode = InputMode::Normal;
                            self.input_buffer.clear();
                            return Ok(());
                        }
                    }
                }
            }
            InputMode::MaxSize => {
                if value.is_empty() {
                    self.filters.max_size = None;
                } else {
                    match parse_size(&value) {
                        Ok(size) => self.filters.max_size = Some(size),
                        Err(e) => {
                            self.status = e;
                            self.input_mode = InputMode::Normal;
                            self.input_buffer.clear();
                            return Ok(());
                        }
                    }
                }
            }
            InputMode::Normal | InputMode::SortSelect | InputMode::ListPreview => {}
        }

        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.reload()
    }

    /// Clear all filters
    fn clear_filters(&mut self) -> Result<()> {
        self.filters.clear();
        self.reload()
    }

    fn select_next(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => (i + 1).min(self.entries.len() - 1),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_prev(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        let i = match self.list_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        self.list_state.select(Some(i));
    }

    fn select_first(&mut self) {
        if !self.entries.is_empty() {
            self.list_state.select(Some(0));
        }
    }

    fn select_last(&mut self) {
        if !self.entries.is_empty() {
            self.list_state.select(Some(self.entries.len() - 1));
        }
    }

    fn open_list_preview(&mut self) {
        let Some(entry) = list_preview_entry(&self.entries, self.list_state.selected()) else {
            return;
        };
        // Clone before load: `entry` borrows `self.entries`.
        let path = entry.path.clone();
        let size = entry.total_size;
        let atime = entry.latest_atime;
        self.list_preview = Some(Self::load_file_preview(&path, size, atime));
        self.input_mode = InputMode::ListPreview;
    }

    fn close_list_preview(&mut self) {
        self.list_preview = None;
        self.input_mode = InputMode::Normal;
    }

    fn enter_selected(&mut self) -> Result<()> {
        let Some(idx) = self.list_state.selected() else {
            return Ok(());
        };

        let entry = &self.entries[idx];

        if !entry.is_dir {
            return Ok(());
        }

        if entry.name == ".." {
            return self.go_up();
        }

        // If we're at the partition list, enter the partition
        if self.current_partition.is_none() {
            self.current_partition = Some(entry.name.clone());
            self.partition_root.clear();
            self.current_path.clear();
            self.load_directory()?;
        } else {
            // Enter subdirectory - use the full path from the entry
            self.current_path = format!("{}/{}", self.current_path, entry.name);
            self.load_directory()?;
        }

        self.list_state.select(Some(0));
        Ok(())
    }

    fn go_up(&mut self) -> Result<()> {
        if self.current_partition.is_none() {
            // Already at root
            return Ok(());
        }

        if self.current_path == self.partition_root || self.current_path.is_empty() {
            // Go back to partition list
            self.current_partition = None;
            self.partition_root.clear();
            self.current_path.clear();
            self.load_partitions()?;
        } else {
            // Go up one directory
            if let Some(pos) = self.current_path.rfind('/') {
                self.current_path = self.current_path[..pos].to_string();
            } else {
                self.current_path.clear();
            }
            self.load_directory()?;
        }

        self.list_state.select(Some(0));
        Ok(())
    }

    // ---- Tree (Miller columns) mode ----

    /// Create a Column for the partition list.
    fn make_partition_column(&self) -> Result<Column> {
        let glob = index_glob(&self.index_path, None);
        let filter_clause = self.filters.to_where_clause();
        // Exclude __root__ — its files are merged as individual entries
        let having_clause = if filter_clause.is_empty() {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}'",
                ROOT_PARTITION
            )
        } else {
            format!(
                "HAVING partition IS NOT NULL AND partition != '' AND partition != '{}' AND SUM(CASE WHEN {} THEN 1 ELSE 0 END) > 0",
                ROOT_PARTITION, filter_clause
            )
        };

        let sql = if self.filters.is_active() {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(CASE WHEN {filter} THEN size ELSE 0 END) as total_size,
                    MAX(CASE WHEN {filter} THEN atime ELSE 0 END) as latest_atime,
                    SUM(CASE WHEN {filter} THEN 1 ELSE 0 END) as file_count
                FROM read_parquet('{glob}', filename=true)
                GROUP BY partition
                {having}
                ORDER BY {order}
                "#,
                filter = filter_clause,
                glob = glob,
                having = having_clause,
                order = self.sort_mode.to_partition_order_by()
            )
        } else {
            format!(
                r#"
                SELECT 
                    regexp_extract(filename, '.*/([^/]+)/[^/]+\.parquet$', 1) as partition,
                    SUM(size) as total_size,
                    MAX(atime) as latest_atime,
                    COUNT(*) as file_count
                FROM read_parquet('{glob}', filename=true)
                GROUP BY partition
                HAVING partition IS NOT NULL AND partition != '' AND partition != '{root}'
                ORDER BY {order}
                "#,
                glob = glob,
                root = ROOT_PARTITION,
                order = self.sort_mode.to_partition_order_by()
            )
        };

        let mut entries = Vec::new();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let name: String = row.get(0)?;
            let total_size: i64 = row.get(1)?;
            let latest_atime: i64 = row.get(2)?;
            let file_count: i64 = row.get(3)?;
            entries.push(DirEntry {
                name: name.clone(),
                path: name,
                is_dir: true,
                total_size,
                file_count,
                latest_atime,
            });
        }

        // Merge individual files from the __root__ partition
        let root_files = self.query_root_files()?;
        if !root_files.is_empty() {
            entries.extend(root_files);
            Self::sort_entries(&mut entries, self.sort_mode);
        }

        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }

        Ok(Column {
            title: "Partitions".to_string(),
            partition: None,
            partition_root: String::new(),
            path: String::new(),
            entries,
            list_state,
        })
    }

    /// Create a Column for a partition's root directory.
    fn make_partition_root_column(&self, partition: &str) -> Result<Column> {
        let partition_root = self.discover_partition_root(partition)?;
        self.make_directory_column_tree(partition, &partition_root, &partition_root)
    }

    /// Create a Column for a directory within a partition.
    fn make_directory_column_tree(
        &self,
        partition: &str,
        partition_root: &str,
        path: &str,
    ) -> Result<Column> {
        let glob = index_glob(&self.index_path, Some(partition));
        let prefix = format!("{}/", path);
        let prefix_len = prefix.len();

        let filter_clause = self.filters.to_where_clause();
        let file_filter = if filter_clause.is_empty() {
            format!("path LIKE '{}%'", prefix)
        } else {
            format!("path LIKE '{}%' AND {}", prefix, filter_clause)
        };

        let order_by = self.sort_mode.to_order_by(self.sort_mode == SortMode::Name);

        let sql = format!(
            r#"
            WITH files AS (
                SELECT path, size, atime
                FROM read_parquet('{glob}')
                WHERE {file_filter}
            ),
            components AS (
                SELECT 
                    path, size, atime,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 
                        THEN substr(path, {prefix_len} + 1, position('/' IN substr(path, {prefix_len} + 1)) - 1)
                        ELSE substr(path, {prefix_len} + 1)
                    END as component,
                    CASE 
                        WHEN position('/' IN substr(path, {prefix_len} + 1)) > 0 THEN true
                        ELSE false
                    END as is_dir
                FROM files
            )
            SELECT 
                component,
                bool_or(is_dir) as is_dir,
                SUM(size) as total_size,
                COUNT(*) as file_count,
                MAX(atime) as latest_atime
            FROM components
            WHERE component != '' AND component IS NOT NULL
            GROUP BY component
            ORDER BY {order_by}
            "#,
            glob = glob,
            file_filter = file_filter,
            prefix_len = prefix_len,
            order_by = order_by
        );

        let mut entries = Vec::new();
        let mut stmt = self.conn.prepare(&sql)?;
        let mut rows = stmt.query([])?;

        while let Some(row) = rows.next()? {
            let component: String = row.get(0)?;
            let is_dir: bool = row.get(1)?;
            let total_size: i64 = row.get(2)?;
            let file_count: i64 = row.get(3)?;
            let latest_atime: i64 = row.get(4)?;
            entries.push(DirEntry {
                name: component.clone(),
                path: format!("{}/{}", path, component),
                is_dir,
                total_size,
                file_count,
                latest_atime,
            });
        }

        let title = path.rsplit('/').next().unwrap_or(partition).to_string();

        let mut list_state = ListState::default();
        if !entries.is_empty() {
            list_state.select(Some(0));
        }

        Ok(Column {
            title,
            partition: Some(partition.to_string()),
            partition_root: partition_root.to_string(),
            path: path.to_string(),
            entries,
            list_state,
        })
    }

    /// Initialize tree mode columns from scratch.
    fn init_tree(&mut self) -> Result<()> {
        self.columns.clear();
        self.active_column = 0;

        let partition_col = self.make_partition_column()?;
        self.columns.push(partition_col);

        // Auto-expand preview for the selected partition
        self.tree_update_preview()?;
        Ok(())
    }

    /// Update the preview column (active_column + 1) based on the active column's selection.
    /// Truncates any columns beyond the preview. For files, populates `file_preview`.
    fn tree_update_preview(&mut self) -> Result<()> {
        // Remove everything after the active column
        self.columns.truncate(self.active_column + 1);
        self.file_preview = None;
        self.preview_focused = false;

        // Extract info from active column's selection (avoid borrow conflict)
        enum PreviewAction {
            Directory {
                is_partition_list: bool,
                name: String,
                partition: Option<String>,
                partition_root: String,
                path: String,
            },
            File {
                path: String,
                size: i64,
                atime: i64,
            },
            None,
        }

        let action = {
            let col = &self.columns[self.active_column];
            if let Some(idx) = col.list_state.selected() {
                if idx < col.entries.len() {
                    let entry = &col.entries[idx];
                    if entry.is_dir {
                        PreviewAction::Directory {
                            is_partition_list: col.partition.is_none(),
                            name: entry.name.clone(),
                            partition: col.partition.clone(),
                            partition_root: col.partition_root.clone(),
                            path: col.path.clone(),
                        }
                    } else {
                        PreviewAction::File {
                            path: entry.path.clone(),
                            size: entry.total_size,
                            atime: entry.latest_atime,
                        }
                    }
                } else {
                    PreviewAction::None
                }
            } else {
                PreviewAction::None
            }
        };

        match action {
            PreviewAction::Directory {
                is_partition_list,
                name,
                partition,
                partition_root,
                path,
            } => {
                let new_col = if is_partition_list {
                    self.make_partition_root_column(&name)?
                } else {
                    let part = partition.as_ref().unwrap();
                    let new_path = format!("{}/{}", path, name);
                    self.make_directory_column_tree(part, &partition_root, &new_path)?
                };
                self.columns.push(new_col);
            }
            PreviewAction::File { path, size, atime } => {
                self.file_preview = Some(Self::load_file_preview(&path, size, atime));
            }
            PreviewAction::None => {}
        }

        Ok(())
    }

    /// Load file type info and (optionally) text content for the preview pane.
    fn load_file_preview(path: &str, size: i64, atime: i64) -> FilePreview {
        const SNIFF_SIZE: usize = 8192;

        // Read the first SNIFF_SIZE bytes for type detection
        let sniff_buf = File::open(path)
            .and_then(|mut f| {
                let mut buf = vec![0u8; SNIFF_SIZE];
                let n = f.read(&mut buf)?;
                buf.truncate(n);
                Ok(buf)
            })
            .unwrap_or_default();

        if sniff_buf.is_empty() {
            return FilePreview {
                path: path.to_string(),
                size,
                atime,
                type_description: "(unreadable)".to_string(),
                is_text: false,
                lines: VecDeque::new(),
                first_line_number: 0,
                total_lines_loaded: 0,
                file_offset: 0,
                eof_reached: true,
                scroll_offset: 0,
            };
        }

        // Detect file type using multi-layer heuristics
        let (type_description, is_text) = detect_file_type(&sniff_buf, path);

        // Load initial text content if applicable
        let mut lines = VecDeque::new();
        let mut file_offset: u64 = 0;
        let mut eof_reached = !is_text;
        let mut total_lines_loaded: usize = 0;

        if is_text {
            if let Ok(mut f) = File::open(path) {
                let mut buf = vec![0u8; CHUNK_BYTES];
                match f.read(&mut buf) {
                    Ok(0) => {
                        eof_reached = true;
                    }
                    Ok(n) => {
                        file_offset = n as u64;
                        // Check if we hit EOF (read less than buffer)
                        if n < CHUNK_BYTES {
                            eof_reached = true;
                        }
                        // Parse lines from the chunk. If not EOF, the last
                        // "line" may be a partial line — we back up file_offset
                        // to re-read it on the next chunk.
                        let chunk = &buf[..n];
                        if let Ok(text) = std::str::from_utf8(chunk) {
                            let mut iter = text.split('\n').peekable();
                            while let Some(line) = iter.next() {
                                if iter.peek().is_none() && !eof_reached {
                                    // Last fragment before EOF unknown — back up
                                    file_offset -= line.len() as u64;
                                    break;
                                }
                                lines.push_back(strip_ansi(line));
                                total_lines_loaded += 1;
                            }
                        }
                    }
                    Err(_) => {
                        eof_reached = true;
                    }
                }
            }
        }

        FilePreview {
            path: path.to_string(),
            size,
            atime,
            type_description,
            is_text,
            lines,
            first_line_number: 0,
            total_lines_loaded,
            file_offset,
            eof_reached,
            scroll_offset: 0,
        }
    }

    /// Read the next chunk of lines from the file, appending to the deque.
    /// Trims the front if the buffer exceeds MAX_BUFFER_LINES.
    fn preview_load_more_lines(preview: &mut FilePreview) {
        if preview.eof_reached || !preview.is_text {
            return;
        }

        let Ok(mut f) = File::open(&preview.path) else {
            preview.eof_reached = true;
            return;
        };
        if f.seek(SeekFrom::Start(preview.file_offset)).is_err() {
            preview.eof_reached = true;
            return;
        }

        let mut reader = BufReader::with_capacity(CHUNK_BYTES, f);
        let mut bytes_read: u64 = 0;
        let mut line_buf = String::new();

        loop {
            line_buf.clear();
            match reader.read_line(&mut line_buf) {
                Ok(0) => {
                    // True EOF
                    preview.eof_reached = true;
                    break;
                }
                Ok(n) => {
                    bytes_read += n as u64;

                    // Strip trailing newline (and CR if present)
                    if line_buf.ends_with('\n') {
                        line_buf.pop();
                        if line_buf.ends_with('\r') {
                            line_buf.pop();
                        }
                    }

                    preview.lines.push_back(strip_ansi(&line_buf));
                    preview.total_lines_loaded += 1;

                    // Trim front if over budget
                    if preview.lines.len() > MAX_BUFFER_LINES {
                        preview.lines.pop_front();
                        preview.first_line_number += 1;
                    }

                    // Stop after reading ~CHUNK_BYTES
                    if bytes_read >= CHUNK_BYTES as u64 {
                        break;
                    }
                }
                Err(_) => {
                    preview.eof_reached = true;
                    break;
                }
            }
        }

        preview.file_offset += bytes_read;
    }

    /// Read forward in chunks until EOF, maintaining the sliding window.
    fn preview_load_to_eof(preview: &mut FilePreview) {
        while !preview.eof_reached {
            Self::preview_load_more_lines(preview);
        }
    }

    /// Reload the file preview from the beginning (for `g` after front eviction).
    fn preview_reload_from_start(preview: &mut FilePreview) {
        preview.lines.clear();
        preview.first_line_number = 0;
        preview.total_lines_loaded = 0;
        preview.file_offset = 0;
        preview.eof_reached = false;
        preview.scroll_offset = 0;
        Self::preview_load_more_lines(preview);
    }

    /// Move selection down in the active tree column and update preview.
    fn tree_select_next(&mut self) -> Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }
        let col = &mut self.columns[self.active_column];
        if col.entries.is_empty() {
            return Ok(());
        }
        let i = match col.list_state.selected() {
            Some(i) => (i + 1).min(col.entries.len() - 1),
            None => 0,
        };
        col.list_state.select(Some(i));
        self.tree_update_preview()
    }

    /// Move selection up in the active tree column and update preview.
    fn tree_select_prev(&mut self) -> Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }
        let col = &mut self.columns[self.active_column];
        if col.entries.is_empty() {
            return Ok(());
        }
        let i = match col.list_state.selected() {
            Some(i) => i.saturating_sub(1),
            None => 0,
        };
        col.list_state.select(Some(i));
        self.tree_update_preview()
    }

    /// Jump to the first entry in the active tree column.
    fn tree_select_first(&mut self) -> Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }
        let col = &mut self.columns[self.active_column];
        if !col.entries.is_empty() {
            col.list_state.select(Some(0));
        }
        self.tree_update_preview()
    }

    /// Jump to the last entry in the active tree column.
    fn tree_select_last(&mut self) -> Result<()> {
        if self.columns.is_empty() {
            return Ok(());
        }
        let col = &mut self.columns[self.active_column];
        if !col.entries.is_empty() {
            col.list_state.select(Some(col.entries.len() - 1));
        }
        self.tree_update_preview()
    }

    /// Move focus right into the preview column.
    fn tree_right(&mut self) -> Result<()> {
        if self.active_column + 1 < self.columns.len() {
            self.active_column += 1;
            self.tree_update_preview()?;
        }
        Ok(())
    }

    /// Move focus left to the parent column.
    fn tree_left(&mut self) -> Result<()> {
        if self.active_column > 0 {
            self.active_column -= 1;
            // Keep active column + its preview, truncate the rest
            self.columns.truncate(self.active_column + 2);
        }
        Ok(())
    }

    /// Toggle between list and tree view modes, preserving navigation position.
    fn toggle_view_mode(&mut self) -> Result<()> {
        match self.view_mode {
            ViewMode::List => {
                self.view_mode = ViewMode::Tree;
                self.init_tree_from_list_position()?;
            }
            ViewMode::Tree => {
                self.view_mode = ViewMode::List;
                self.restore_list_from_tree_position()?;
            }
        }
        Ok(())
    }

    /// Initialize tree mode, expanding to match the current list-mode position.
    fn init_tree_from_list_position(&mut self) -> Result<()> {
        self.columns.clear();
        self.active_column = 0;

        let partition_col = self.make_partition_column()?;
        self.columns.push(partition_col);

        let Some(ref partition) = self.current_partition else {
            // At partition list — just show the preview for first partition
            self.tree_update_preview()?;
            return Ok(());
        };
        let partition = partition.clone();

        // Select the matching partition in column 0
        if let Some(idx) = self.columns[0]
            .entries
            .iter()
            .position(|e| e.name == partition)
        {
            self.columns[0].list_state.select(Some(idx));
        }

        // Expand into partition root
        self.tree_update_preview()?;
        self.active_column = 1;

        // Walk path components from partition_root to current_path
        if !self.current_path.is_empty() && self.current_path.len() > self.partition_root.len() {
            let relative = self.current_path[self.partition_root.len()..].to_string();
            let components: Vec<&str> = relative.split('/').filter(|c| !c.is_empty()).collect();

            for component in &components {
                // Select the matching entry in the current active column
                let col = &self.columns[self.active_column];
                if let Some(idx) = col.entries.iter().position(|e| e.name == *component) {
                    self.columns[self.active_column]
                        .list_state
                        .select(Some(idx));
                    self.tree_update_preview()?;
                    // Move into the newly expanded column
                    if self.active_column + 1 < self.columns.len() {
                        self.active_column += 1;
                    }
                } else {
                    break;
                }
            }
        }

        // Active column is the deepest directory we reached; update its preview
        self.tree_update_preview()?;
        Ok(())
    }

    /// Restore list-mode state from the current tree-mode position.
    fn restore_list_from_tree_position(&mut self) -> Result<()> {
        if self.columns.is_empty() {
            // No tree state — fall back to partition list
            self.current_partition = None;
            self.partition_root.clear();
            self.current_path.clear();
            self.load_partitions()?;
            if !self.entries.is_empty() {
                self.list_state.select(Some(0));
            }
            return Ok(());
        }

        let col = &self.columns[self.active_column];
        let selected_name = col
            .list_state
            .selected()
            .and_then(|idx| col.entries.get(idx))
            .map(|e| e.name.clone());

        if col.partition.is_none() {
            // Active column is the partition list
            self.current_partition = None;
            self.partition_root.clear();
            self.current_path.clear();
            self.load_partitions()?;
            // Try to select the same partition
            if let Some(name) = selected_name {
                if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                    self.list_state.select(Some(idx));
                    return Ok(());
                }
            }
            if !self.entries.is_empty() {
                self.list_state.select(Some(0));
            }
        } else {
            // Active column is inside a partition
            self.current_partition = col.partition.clone();
            self.partition_root = col.partition_root.clone();
            self.current_path = col.path.clone();
            self.load_directory()?;
            // Try to select the same entry
            if let Some(name) = selected_name {
                if let Some(idx) = self.entries.iter().position(|e| e.name == name) {
                    self.list_state.select(Some(idx));
                    return Ok(());
                }
            }
            if !self.entries.is_empty() {
                self.list_state.select(Some(0));
            }
        }

        Ok(())
    }

    fn format_atime(&self, atime: i64) -> String {
        if atime == 0 {
            return String::new();
        }

        use std::time::{Duration, SystemTime, UNIX_EPOCH};
        let time = UNIX_EPOCH + Duration::from_secs(atime as u64);
        let now = SystemTime::now();

        if let Ok(duration) = now.duration_since(time) {
            let days = duration.as_secs() / 86400;
            if days == 0 {
                "today".to_string()
            } else if days == 1 {
                "1 day ago".to_string()
            } else if days < 30 {
                format!("{} days ago", days)
            } else if days < 365 {
                let months = days / 30;
                if months == 1 {
                    "1 month ago".to_string()
                } else {
                    format!("{} months ago", months)
                }
            } else {
                let years = days / 365;
                if years == 1 {
                    "1 year ago".to_string()
                } else {
                    format!("{} years ago", years)
                }
            }
        } else {
            "future".to_string()
        }
    }
}

fn main() -> Result<()> {
    let args = XduViewArgs::parse();

    // Resolve index path
    let index_path = args
        .index
        .canonicalize()
        .with_context(|| format!("Index directory not found: {}", args.index.display()))?;

    // Refuse an index whose format version is unknown or unsupported before the
    // terminal is touched, so the diagnostic survives on the terminal instead of being
    // wiped with the alternate screen.
    if let Some(error) = index_version_error(&index_path) {
        return Err(anyhow::anyhow!(error));
    }

    // Warn before the alternate screen takes over, so the message is still on the
    // terminal after the TUI exits rather than being wiped with the screen.
    if let Some(warning) = index_completion_warning(&index_path) {
        eprintln!("{}", warning);
    }

    // Parse sort mode
    let sort_mode: SortMode = args.sort.parse().map_err(|e: String| anyhow::anyhow!(e))?;

    // Build filters from CLI args. This runs before the terminal is touched, so an
    // invalid pattern exits without ever owning raw mode or the alternate screen.
    let filters = QueryFilters::new()
        .with_path_pattern(args.pattern, args.regex)
        .map_err(|e| anyhow::anyhow!(e))?
        .with_older_than(args.older_than)
        .with_newer_than(args.newer_than)
        .with_min_size(args.min_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?
        .with_max_size(args.max_size.as_deref())
        .map_err(|e| anyhow::anyhow!(e))?;

    // Connect to DuckDB
    let conn = xdu::engine::open_connection()?;

    // Initialize app
    let mut app = App::new(conn, index_path, args.partition, filters, sort_mode)?;

    // Setup terminal
    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout()))?;

    // Main loop
    let result = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;

    result
}

fn run_app<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Handle sort selection mode
                if app.input_mode == InputMode::SortSelect {
                    match key.code {
                        KeyCode::Esc => app.cancel_sort(),
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            if let Err(e) = app.confirm_sort() {
                                app.status = format!("Error: {}", e);
                            }
                        }
                        KeyCode::Up | KeyCode::Char('k') | KeyCode::Left => {
                            app.sort_select_prev();
                        }
                        KeyCode::Down
                        | KeyCode::Char('j')
                        | KeyCode::Right
                        | KeyCode::Char('s') => {
                            app.sort_select_next();
                        }
                        _ => {}
                    }
                    continue;
                }

                if app.input_mode == InputMode::ListPreview {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q') => {
                            app.close_list_preview();
                        }
                        _ => {}
                    }
                    continue;
                }

                // Handle text input mode
                if app.input_mode != InputMode::Normal {
                    match key.code {
                        KeyCode::Esc => app.cancel_input(),
                        KeyCode::Enter => {
                            if let Err(e) = app.confirm_input() {
                                app.status = format!("Error: {}", e);
                            }
                        }
                        KeyCode::Backspace => {
                            app.input_buffer.pop();
                        }
                        KeyCode::Char(c) => {
                            app.input_buffer.push(c);
                        }
                        _ => {}
                    }
                    continue;
                }

                // Handle preview pager focus mode (tree view file preview)
                if app.preview_focused {
                    match key.code {
                        KeyCode::Char('j') | KeyCode::Down => {
                            if let Some(ref mut preview) = app.file_preview {
                                preview.scroll_offset += 1;
                                // Load more if we're near the end of loaded content
                                let last_loaded = preview.first_line_number + preview.lines.len();
                                if preview.scroll_offset + 40 >= last_loaded {
                                    App::preview_load_more_lines(preview);
                                }
                                // Clamp to total loaded
                                let max_offset = preview.total_lines_loaded.saturating_sub(1);
                                preview.scroll_offset = preview.scroll_offset.min(max_offset);
                            }
                        }
                        KeyCode::Char('k') | KeyCode::Up => {
                            if let Some(ref mut preview) = app.file_preview {
                                if preview.scroll_offset > 0 {
                                    preview.scroll_offset -= 1;
                                }
                                // If scrolled before the buffer start, clamp
                                if preview.scroll_offset < preview.first_line_number {
                                    preview.scroll_offset = preview.first_line_number;
                                }
                            }
                        }
                        KeyCode::Char('g') => {
                            if let Some(ref mut preview) = app.file_preview {
                                if preview.first_line_number > 0 {
                                    // Front was evicted, reload from start
                                    App::preview_reload_from_start(preview);
                                } else {
                                    preview.scroll_offset = 0;
                                }
                            }
                        }
                        KeyCode::Char('G') => {
                            if let Some(ref mut preview) = app.file_preview {
                                App::preview_load_to_eof(preview);
                                preview.scroll_offset =
                                    preview.total_lines_loaded.saturating_sub(1);
                            }
                        }
                        KeyCode::Char('d') => {
                            // Half-page down
                            if let Some(ref mut preview) = app.file_preview {
                                preview.scroll_offset += 20;
                                // Load more if needed
                                let last_loaded = preview.first_line_number + preview.lines.len();
                                if preview.scroll_offset + 40 >= last_loaded && !preview.eof_reached
                                {
                                    App::preview_load_more_lines(preview);
                                }
                                let max_offset = preview.total_lines_loaded.saturating_sub(1);
                                preview.scroll_offset = preview.scroll_offset.min(max_offset);
                            }
                        }
                        KeyCode::Char('u') => {
                            // Half-page up
                            if let Some(ref mut preview) = app.file_preview {
                                preview.scroll_offset = preview.scroll_offset.saturating_sub(20);
                                if preview.scroll_offset < preview.first_line_number {
                                    preview.scroll_offset = preview.first_line_number;
                                }
                            }
                        }
                        KeyCode::Esc | KeyCode::Left | KeyCode::Char('q') | KeyCode::Char(' ') => {
                            app.preview_focused = false;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Normal mode — shared keys first, then mode-specific navigation
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    // Toggle view mode
                    KeyCode::Char('t') => {
                        if let Err(e) = app.toggle_view_mode() {
                            app.status = format!("Error: {}", e);
                        }
                    }
                    // Sort mode selection (both modes)
                    KeyCode::Char('s') => app.start_sort_select(),
                    // Filter inputs (both modes)
                    KeyCode::Char('/') => app.start_input(InputMode::Pattern),
                    KeyCode::Char('o') => app.start_input(InputMode::OlderThan),
                    KeyCode::Char('n') => app.start_input(InputMode::NewerThan),
                    KeyCode::Char('>') => app.start_input(InputMode::MinSize),
                    KeyCode::Char('<') => app.start_input(InputMode::MaxSize),
                    // Clear filters (both modes)
                    KeyCode::Char('c') => {
                        if let Err(e) = app.clear_filters() {
                            app.status = format!("Error: {}", e);
                        }
                    }
                    // Mode-specific navigation
                    _ => match app.view_mode {
                        ViewMode::List => match key.code {
                            KeyCode::Down | KeyCode::Char('j') => app.select_next(),
                            KeyCode::Up | KeyCode::Char('k') => app.select_prev(),
                            KeyCode::Char('g') => app.select_first(),
                            KeyCode::Char('G') => app.select_last(),
                            KeyCode::Enter | KeyCode::Right => {
                                if let Err(e) = app.enter_selected() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Char(' ') => {
                                app.open_list_preview();
                            }
                            KeyCode::Left | KeyCode::Backspace => {
                                if let Err(e) = app.go_up() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            _ => {}
                        },
                        ViewMode::Tree => match key.code {
                            KeyCode::Down | KeyCode::Char('j') => {
                                if let Err(e) = app.tree_select_next() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => {
                                if let Err(e) = app.tree_select_prev() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Char('g') => {
                                if let Err(e) = app.tree_select_first() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Char('G') => {
                                if let Err(e) = app.tree_select_last() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Right | KeyCode::Enter | KeyCode::Char(' ') => {
                                // If a file is selected, activate pager mode
                                if app.file_preview.is_some() {
                                    app.preview_focused = true;
                                } else if let Err(e) = app.tree_right() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            KeyCode::Left | KeyCode::Backspace => {
                                if let Err(e) = app.tree_left() {
                                    app.status = format!("Error: {}", e);
                                }
                            }
                            _ => {}
                        },
                    },
                }
            }
        }
    }
}

fn ui(f: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(f.area());

    // Render content based on view mode
    match app.view_mode {
        ViewMode::List => {
            render_list_content(f, app, chunks[0]);
            if app.list_preview.is_some() {
                render_list_preview_overlay(f, app, chunks[0]);
            }
        }
        ViewMode::Tree => render_tree_content(f, app, chunks[0]),
    }

    // Shared status bar
    render_status_bar(f, app, chunks[1]);
}

/// Render the traditional single-list view.
fn render_list_content(f: &mut Frame, app: &App, area: Rect) {
    // Build filter display string
    let filter_display = app.filters.format_display();
    let filter_suffix = if filter_display.is_empty() {
        String::new()
    } else {
        format!(" {}", filter_display)
    };

    // Title with current path
    let title = if let Some(ref partition) = app.current_partition {
        let display_path = if app.current_path.len() > app.partition_root.len() {
            &app.current_path[app.partition_root.len()..]
        } else {
            ""
        };
        if app.loading {
            format!(
                " {}{} (loading...){} ",
                partition, display_path, filter_suffix
            )
        } else {
            format!(" {}{}{} ", partition, display_path, filter_suffix)
        }
    } else {
        if app.loading {
            format!(" Partitions (loading...){} ", filter_suffix)
        } else {
            format!(" Partitions{} ", filter_suffix)
        }
    };

    // Pre-compute formatted strings and find max widths dynamically
    let formatted: Vec<(String, String, String, String)> = app
        .entries
        .iter()
        .map(|entry| {
            let prefix = if entry.is_dir && entry.name != ".." {
                "▸ "
            } else if entry.name == ".." {
                "◂ "
            } else {
                "  "
            };

            let name = format!("{}{}", prefix, entry.name);

            let size_str = if entry.name == ".." {
                String::new()
            } else {
                format_bytes(entry.total_size as u64)
            };

            let count_str = if entry.name == ".." {
                String::new()
            } else {
                format_file_count(entry.file_count)
            };

            let atime_str = if entry.name == ".." {
                String::new()
            } else {
                app.format_atime(entry.latest_atime)
            };

            (name, size_str, count_str, atime_str)
        })
        .collect();

    // Calculate dynamic column widths based on content (with minimum widths)
    let size_width = formatted
        .iter()
        .map(|(_, s, _, _)| s.len())
        .max()
        .unwrap_or(0)
        .max(10);
    let count_width = formatted
        .iter()
        .map(|(_, _, c, _)| c.len())
        .max()
        .unwrap_or(0)
        .max(8);
    let atime_width = formatted
        .iter()
        .map(|(_, _, _, a)| a.len())
        .max()
        .unwrap_or(0)
        .max(12);

    // Calculate available width for names
    let area_width = area.width.saturating_sub(2) as usize; // Account for borders
    let fixed_cols = size_width + count_width + atime_width + 8; // 8 = spacing between columns + highlight symbol
    let name_width = area_width.saturating_sub(fixed_cols);

    // Entry list
    let items: Vec<ListItem> = formatted
        .iter()
        .map(|(name, size_str, count_str, atime_str)| {
            let name_display = if name.len() > name_width {
                format!("{}…", &name[..name_width.saturating_sub(1)])
            } else {
                name.clone()
            };

            let line = format!(
                "{:<name_width$}  {:>size_width$}  {:>count_width$}  {:>atime_width$}",
                name_display,
                size_str,
                count_str,
                atime_str,
                name_width = name_width,
                size_width = size_width,
                count_width = count_width,
                atime_width = atime_width
            );

            ListItem::new(line)
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_symbol("▶ ")
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

    f.render_stateful_widget(list, area, &mut app.list_state.clone());
}

/// Minimum column width for Miller columns view.
const MIN_COLUMN_WIDTH: u16 = 20;
/// Maximum number of visible columns.
const MAX_VISIBLE_COLUMNS: usize = 5;

/// Render the Miller columns (tree) view.
fn render_tree_content(f: &mut Frame, app: &App, area: Rect) {
    if app.columns.is_empty() {
        return;
    }

    // Determine whether the rightmost slot is a file preview or a directory column.
    // The preview pane occupies one slot regardless.
    let has_preview_pane = app.file_preview.is_some() || app.columns.len() > app.active_column + 1; // dir preview column exists
    let need_extra_slot = app.file_preview.is_some(); // file preview needs its own slot

    // Total pane count: directory columns + (1 extra if file preview occupies a separate slot)
    let total_panes = app.columns.len() + if need_extra_slot { 1 } else { 0 };

    let available_width = area.width;
    let num_visible = total_panes
        .min(MAX_VISIBLE_COLUMNS)
        .min((available_width / MIN_COLUMN_WIDTH) as usize)
        .max(1);

    // Ensure the active column + its preview are visible
    let slots_after_active = if has_preview_pane { 2 } else { 1 };
    let start_idx = if app.active_column + slots_after_active > num_visible {
        (app.active_column + slots_after_active)
            .saturating_sub(num_visible)
            .min(total_panes.saturating_sub(num_visible))
    } else {
        0
    };
    let end_idx = (start_idx + num_visible).min(total_panes);
    let actual_visible = end_idx - start_idx;

    // Create horizontal layout with equal widths
    let constraints: Vec<Constraint> = (0..actual_visible)
        .map(|_| Constraint::Ratio(1, actual_visible as u32))
        .collect();

    let col_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(area);

    for slot in 0..actual_visible {
        let global_idx = start_idx + slot;

        // Is this slot the file-preview pane?
        let is_file_preview_slot = need_extra_slot && global_idx == app.columns.len();

        if is_file_preview_slot {
            render_file_preview_pane(f, app, col_chunks[slot]);
            continue;
        }

        // Otherwise render a directory column
        if global_idx >= app.columns.len() {
            continue;
        }
        let col = &app.columns[global_idx];
        let is_active = global_idx == app.active_column;
        let is_dir_preview = global_idx == app.active_column + 1;

        let border_style = if is_active {
            Style::default().fg(Color::Blue)
        } else if is_dir_preview {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let col_inner_width = col_chunks[slot].width.saturating_sub(2) as usize;
        let highlight_width = 2;
        let content_width = col_inner_width.saturating_sub(highlight_width);

        let formatted: Vec<(String, String, String, String)> = col
            .entries
            .iter()
            .map(|entry| {
                let prefix = if entry.is_dir { "▸ " } else { "  " };
                let name = format!("{}{}", prefix, entry.name);
                let size_str = format_bytes(entry.total_size as u64);
                let count_str = format_file_count(entry.file_count);
                let atime_str = app.format_atime(entry.latest_atime);
                (name, size_str, count_str, atime_str)
            })
            .collect();

        let size_w = formatted
            .iter()
            .map(|(_, s, _, _)| s.len())
            .max()
            .unwrap_or(0)
            .max(6);
        let count_w = formatted
            .iter()
            .map(|(_, _, c, _)| c.len())
            .max()
            .unwrap_or(0)
            .max(6);
        let atime_w = formatted
            .iter()
            .map(|(_, _, _, a)| a.len())
            .max()
            .unwrap_or(0)
            .max(6);
        let stat_cols = size_w + count_w + atime_w + 6;
        let name_max = content_width.saturating_sub(stat_cols);

        let items: Vec<ListItem> = formatted
            .iter()
            .map(|(name, size_str, count_str, atime_str)| {
                let name_display = if name.len() > name_max && name_max > 1 {
                    format!("{}…", &name[..name_max.saturating_sub(1)])
                } else if name_max == 0 {
                    String::new()
                } else {
                    name.clone()
                };

                let line = format!(
                    "{:<name_w$}  {:>size_w$}  {:>count_w$}  {:>atime_w$}",
                    name_display,
                    size_str,
                    count_str,
                    atime_str,
                    name_w = name_max,
                    size_w = size_w,
                    count_w = count_w,
                    atime_w = atime_w
                );

                ListItem::new(line)
            })
            .collect();

        let title_style = if is_active {
            Style::default()
                .fg(Color::Blue)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(Span::styled(format!(" {} ", col.title), title_style)),
            )
            .highlight_symbol("▶ ")
            .highlight_style(Style::default().add_modifier(Modifier::REVERSED));

        f.render_stateful_widget(list, col_chunks[slot], &mut col.list_state.clone());
    }
}

/// Render the file preview pane (rightmost slot when a file is selected).
fn render_file_preview_pane(f: &mut Frame, app: &App, area: Rect) {
    // Clear stale content from previous preview before rendering new content.
    // Without this, rows below the last text line can retain old characters
    // because Paragraph only writes cells for its actual lines.
    f.render_widget(Clear, area);

    let Some(ref preview) = app.file_preview else {
        // Empty placeholder pane
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray))
            .title(Span::styled(
                " Preview ",
                Style::default().fg(Color::DarkGray),
            ));
        f.render_widget(block, area);
        return;
    };

    let border_style = if app.preview_focused {
        Style::default().fg(Color::Blue)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let title_style = if app.preview_focused {
        Style::default()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Cyan)
    };

    let file_name = preview.path.rsplit('/').next().unwrap_or(&preview.path);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(format!(" {} ", file_name), title_style));

    let inner_height = area.height.saturating_sub(2) as usize; // borders top+bottom

    // Build content lines: header info + optional text content
    let mut text_lines: Vec<Line> = Vec::new();

    // File metadata header
    text_lines.push(Line::from(Span::styled(
        format!("  Type: {}", preview.type_description),
        Style::default().fg(Color::Yellow),
    )));
    text_lines.push(Line::from(Span::styled(
        format!("  Size: {}", format_bytes(preview.size as u64)),
        Style::default().fg(Color::Green),
    )));
    text_lines.push(Line::from(Span::styled(
        format!("  Atime: {}", app.format_atime(preview.atime)),
        Style::default().fg(Color::Cyan),
    )));
    text_lines.push(Line::from(""));

    if preview.is_text && !preview.lines.is_empty() {
        // Translate global scroll_offset to deque index
        let content_height = inner_height.saturating_sub(text_lines.len());
        let global_start = preview.scroll_offset.max(preview.first_line_number);
        let deque_start = global_start - preview.first_line_number;
        let deque_end = (deque_start + content_height).min(preview.lines.len());

        for i in deque_start..deque_end {
            text_lines.push(Line::from(Span::raw(format!("  {}", preview.lines[i]))));
        }

        // Scroll indicator with global position and EOF status
        let eof_indicator = if preview.eof_reached { "EOF" } else { "..." };
        let total = preview.total_lines_loaded;
        if total > content_height {
            let max_scroll = total.saturating_sub(content_height);
            let pct = (global_start * 100).checked_div(max_scroll).unwrap_or(100);
            text_lines.push(Line::from(Span::styled(
                format!(
                    "  ── {}% ({}/{} lines) ({}) ──",
                    pct.min(100),
                    global_start + 1,
                    total,
                    eof_indicator
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
    } else if !preview.is_text {
        text_lines.push(Line::from(Span::styled(
            "  (binary file — no preview)",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let paragraph = Paragraph::new(text_lines).block(block);
    f.render_widget(paragraph, area);
}

/// Center a popup inside `area`. Percentages are of the parent, not absolute cells.
fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup[1])[1]
}

/// Cut on a char boundary. Byte-index slicing panics on a multibyte name, and
/// ratatui then never restores the terminal.
fn truncate_to_chars(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let n = s.chars().count();
    if n <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().take(keep).collect();
    out.push('…');
    out
}

/// List-mode overlay: type + a short text window over the current list.
fn render_list_preview_overlay(f: &mut Frame, app: &App, area: Rect) {
    let Some(ref preview) = app.list_preview else {
        return;
    };

    let overlay = centered_rect(80, 70, area);
    f.render_widget(Clear, overlay);

    let file_name = preview.path.rsplit('/').next().unwrap_or(&preview.path);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            format!(" {} ", file_name),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));

    let inner = block.inner(overlay);
    let inner_width = inner.width as usize;
    let inner_height = inner.height as usize;

    let mut text_lines: Vec<Line> = Vec::new();
    text_lines.push(Line::from(Span::styled(
        truncate_to_chars(
            &format!("  Type: {}", preview.type_description),
            inner_width,
        ),
        Style::default().fg(Color::Yellow),
    )));
    text_lines.push(Line::from(Span::styled(
        truncate_to_chars(
            &format!("  Size: {}", format_bytes(preview.size as u64)),
            inner_width,
        ),
        Style::default().fg(Color::Green),
    )));
    text_lines.push(Line::from(Span::styled(
        truncate_to_chars(
            &format!("  Atime: {}", app.format_atime(preview.atime)),
            inner_width,
        ),
        Style::default().fg(Color::Cyan),
    )));
    text_lines.push(Line::from(""));

    if preview.is_text && !preview.lines.is_empty() {
        let content_height = inner_height.saturating_sub(text_lines.len());
        for line in preview.lines.iter().take(content_height) {
            text_lines.push(Line::from(Span::raw(truncate_to_chars(
                &format!("  {line}"),
                inner_width,
            ))));
        }
    } else if !preview.is_text {
        text_lines.push(Line::from(Span::styled(
            "  (binary file — no preview)",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    let paragraph = Paragraph::new(text_lines).block(block);
    f.render_widget(paragraph, overlay);
}

/// Render the shared status bar.
fn render_status_bar(f: &mut Frame, app: &App, area: Rect) {
    let status_text = if app.input_mode == InputMode::SortSelect {
        // Build sort selector display with short names
        let options: Vec<String> = SortMode::ALL
            .iter()
            .map(|m| {
                if *m == app.pending_sort {
                    format!("▶ {} ◀", m)
                } else {
                    format!("  {}  ", m)
                }
            })
            .collect();
        format!(
            " Sort: {}  (s/→:next  ←:prev  Enter:apply  Esc:cancel)",
            options.join("")
        )
    } else if app.input_mode == InputMode::ListPreview {
        if let Some(ref preview) = app.list_preview {
            let file_name = preview.path.rsplit('/').next().unwrap_or(&preview.path);
            format!(
                " {} │ {} │ Esc/Space: close",
                file_name, preview.type_description
            )
        } else {
            " Esc/Space: close".to_string()
        }
    } else if app.input_mode != InputMode::Normal {
        format!(" {}{}", app.input_mode.prompt(), app.input_buffer)
    } else if app.preview_focused {
        // Pager mode status
        if let Some(ref preview) = app.file_preview {
            let file_name = preview.path.rsplit('/').next().unwrap_or(&preview.path);
            format!(
                " ▶ {} │ {} │ jk↑↓:scroll d/u:page g/G:top/bottom Esc/←/q/␣:back",
                file_name, preview.type_description
            )
        } else {
            " Esc:back".to_string()
        }
    } else {
        // Build mode-specific status
        let mode_indicator = match app.view_mode {
            ViewMode::List => "list",
            ViewMode::Tree => "tree",
        };

        // In tree mode, show breadcrumb and selected entry info
        let context_info = if app.view_mode == ViewMode::Tree && !app.columns.is_empty() {
            // Build breadcrumb from column titles
            let breadcrumb: Vec<&str> = app
                .columns
                .iter()
                .take(app.active_column + 1)
                .map(|c| c.title.as_str())
                .collect();
            let path_str = breadcrumb.join(" > ");

            // Selected entry details
            let col = &app.columns[app.active_column];
            if let Some(idx) = col.list_state.selected() {
                if idx < col.entries.len() {
                    let entry = &col.entries[idx];
                    let atime_str = app.format_atime(entry.latest_atime);
                    format!(
                        "{} │ {} │ {} │ {}",
                        path_str,
                        format_bytes(entry.total_size as u64),
                        format_file_count(entry.file_count),
                        atime_str
                    )
                } else {
                    path_str
                }
            } else {
                path_str
            }
        } else {
            app.status.clone()
        };

        let filter_display = app.filters.format_display();
        let filter_str = if filter_display.is_empty() {
            String::new()
        } else {
            format!(" {}", filter_display)
        };

        format!(
            " {}{} │ sort:{} mode:{} │ q:quit jk↑↓:nav ←→:cd /:pattern s:sort t:mode c:clear",
            context_info, filter_str, app.sort_mode, mode_indicator
        )
    };

    let status_style = if app.input_mode == InputMode::SortSelect {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if app.input_mode != InputMode::Normal {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().add_modifier(Modifier::DIM)
    };

    let status = Paragraph::new(status_text).style(status_style);
    f.render_widget(status, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool) -> DirEntry {
        DirEntry {
            name: name.to_string(),
            path: name.to_string(),
            is_dir,
            total_size: 0,
            file_count: 0,
            latest_atime: 0,
        }
    }

    #[test]
    fn list_preview_entry_file() {
        let entries = [entry("a.txt", false)];
        let got = list_preview_entry(&entries, Some(0));
        assert_eq!(got.map(|e| e.name.as_str()), Some("a.txt"));
    }

    #[test]
    fn list_preview_entry_directory() {
        let entries = [entry("subdir", true)];
        assert!(list_preview_entry(&entries, Some(0)).is_none());
    }

    #[test]
    fn list_preview_entry_dotdot() {
        let entries = [entry("..", true)];
        assert!(list_preview_entry(&entries, Some(0)).is_none());
    }

    #[test]
    fn list_preview_entry_no_selection() {
        let entries = [entry("a.txt", false)];
        assert!(list_preview_entry(&entries, None).is_none());
    }

    #[test]
    fn list_preview_entry_empty() {
        let entries: [DirEntry; 0] = [];
        assert!(list_preview_entry(&entries, Some(0)).is_none());
    }
}
