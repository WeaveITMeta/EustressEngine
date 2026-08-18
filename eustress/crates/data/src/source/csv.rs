//! CSV / Excel provider.
//!
//! The one provider in the [`super`] family that reads from the local
//! filesystem rather than a network endpoint, and the one that is fully real
//! today: [`CsvSource::fetch`] produces a genuine [`Frame`] with the crate's
//! own dtype and unit inference.
//!
//! ## What "Excel" means here
//!
//! The Data menu labels this provider "CSV / Excel File" and
//! [`SourceKind::parse`] accepts `"excel"` as an alias, so a user can and will
//! point it at an `.xlsx` workbook. Reading a workbook needs a ZIP + OOXML
//! decoder that this leaf deliberately does not carry, so a spreadsheet path is
//! rejected at validation time with an error that says exactly what to do
//! instead (export the sheet to CSV). A loud, honest refusal beats a silent
//! mis-parse of ZIP bytes as text.
//!
//! ## Cost model
//!
//! [`CsvSource::test_connection`] is a probe, not a parse. It makes one
//! buffered byte pass over the file, tracking only whether it sits inside a
//! quoted field, and counts records and header fields. No field is allocated,
//! no UTF-8 is decoded, and no [`Frame`] is built, so probing a 500 MB export
//! costs a sequential read rather than a full import. The counts are exact for
//! well-formed RFC 4180 input, including fields that contain embedded newlines
//! or delimiters.
//!
//! ## Feature gating
//!
//! Validation and probing are pure `std` and always compile. The actual parse
//! reuses [`crate::import::frame_from_csv`], which lives behind the crate's
//! `import` feature; without that feature [`CsvSource::fetch`] returns a clear
//! error naming the missing feature rather than failing to build.

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::{ConnectionStatus, DataSource, SourceConfig, SourceKind};
use crate::{DataError, Frame, Result};

/// Option key selecting the field separator.
pub const DELIMITER_OPTION: &str = "delimiter";

/// Separator used when the config sets no [`DELIMITER_OPTION`].
pub const DEFAULT_DELIMITER: u8 = b',';

/// Workbook extensions this provider refuses, rather than parsing container
/// bytes as delimited text.
const SPREADSHEET_EXTENSIONS: [&str; 5] = ["xlsx", "xlsm", "xlsb", "xls", "ods"];

/// A CSV file on the local filesystem, normalized into the columnar core.
///
/// Construction is infallible so a registry can always hold one; [`validate`]
/// is the gate, and both [`CsvSource::test_connection`] and
/// [`CsvSource::fetch`] run it before touching the disk.
#[derive(Debug, Clone)]
pub struct CsvSource {
    config: SourceConfig,
}

impl CsvSource {
    /// Wrap a config. Does not validate; see [`validate`].
    pub fn new(config: SourceConfig) -> Self {
        Self { config }
    }

    /// The config this source was built from.
    pub fn config(&self) -> &SourceConfig {
        &self.config
    }

    /// The file this source reads.
    pub fn path(&self) -> &Path {
        Path::new(self.config.endpoint.trim())
    }

    /// The configured field separator, or [`DEFAULT_DELIMITER`].
    pub fn delimiter(&self) -> Result<u8> {
        delimiter_of(&self.config)
    }
}

impl DataSource for CsvSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Csv
    }

    fn test_connection(&self) -> Result<ConnectionStatus> {
        // A bad config is the caller's error, so it surfaces as `Err`. Anything
        // about the file itself (absent, unreadable, empty) is a fact the probe
        // successfully established, so it surfaces as an unreachable status the
        // UI can render next to the path.
        validate(&self.config)?;
        let delimiter = delimiter_of(&self.config)?;
        let path = self.path();

        let meta = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ConnectionStatus::failed(format!(
                    "no file at '{}'",
                    path.display()
                )));
            }
            Err(e) => {
                return Ok(ConnectionStatus::failed(format!(
                    "cannot read '{}': {e}",
                    path.display()
                )));
            }
        };
        if meta.is_dir() {
            return Ok(ConnectionStatus::failed(format!(
                "'{}' is a directory, not a CSV file",
                path.display()
            )));
        }

        let counts = match probe_counts(path, delimiter) {
            Ok(c) => c,
            Err(DataError::Io(e)) => {
                return Ok(ConnectionStatus::failed(format!(
                    "cannot read '{}': {e}",
                    path.display()
                )));
            }
            Err(other) => return Err(other),
        };

        if counts.records == 0 {
            return Ok(ConnectionStatus::failed(format!(
                "'{}' is empty; expected at least a header row",
                path.display()
            )));
        }

        let data_rows = counts.records - 1;
        let detail = if data_rows == 0 {
            format!("0 data rows, {} columns (header only)", counts.header_fields)
        } else {
            format!("{data_rows} data rows, {} columns", counts.header_fields)
        };
        Ok(ConnectionStatus::ok(detail))
    }

    fn fetch(&self) -> Result<Frame> {
        validate(&self.config)?;
        let delimiter = delimiter_of(&self.config)?;
        let path = self.path();

        // Distinguish the three ways a path can disappoint before handing the
        // bytes to the parser, so the message names the cause rather than
        // bottoming out in "csv: no header columns".
        let meta = std::fs::metadata(path).map_err(|e| {
            DataError::Io(std::io::Error::new(
                e.kind(),
                format!("CSV source: cannot read '{}': {e}", path.display()),
            ))
        })?;
        if meta.is_dir() {
            return Err(DataError::Schema(format!(
                "CSV source: '{}' is a directory, not a CSV file",
                path.display()
            )));
        }
        if !has_header_row(path)? {
            return Err(DataError::Schema(format!(
                "CSV source: '{}' is empty; expected at least a header row",
                path.display()
            )));
        }

        read_frame(path, delimiter)
    }
}

/// Validate a CSV config without touching the disk.
///
/// Runs the shared [`super::validate_config`] checks first (non-empty
/// endpoint, not a URL, sane `poll_seconds` and `secret_ref`), then the two
/// checks only this provider can make: the delimiter must be a single
/// separator byte, and the path must not be a spreadsheet workbook.
///
/// Pure: no filesystem access, so the UI can call it on every keystroke.
pub fn validate(config: &SourceConfig) -> Result<()> {
    if config.kind != SourceKind::Csv {
        return Err(DataError::Schema(format!(
            "CsvSource cannot serve a {} config",
            config.kind.as_str()
        )));
    }
    super::validate_config(config)?;
    delimiter_of(config)?;

    let path = config.endpoint.trim();
    if let Some(ext) = spreadsheet_extension(path) {
        return Err(DataError::Schema(format!(
            "CSV source: '{path}' is an Excel workbook (.{ext}); only delimited text \
             (CSV, TSV) is supported today. Export the sheet to CSV and point the \
             source at that file."
        )));
    }
    Ok(())
}

/// Resolve the [`DELIMITER_OPTION`] to a single separator byte.
pub fn delimiter_of(config: &SourceConfig) -> Result<u8> {
    match config.option(DELIMITER_OPTION) {
        Some(raw) => parse_delimiter(raw),
        None => Ok(DEFAULT_DELIMITER),
    }
}

/// Accept either a literal one-byte separator (`;`, a tab character, a space)
/// or a written-out name (`tab`, `\t`, `comma`, `semicolon`, `pipe`, `space`),
/// so a hand-edited `_instance.toml` stays readable and an invisible tab is
/// never required.
fn parse_delimiter(raw: &str) -> Result<u8> {
    // The literal form is checked first and un-trimmed, so a deliberate single
    // space survives.
    let bytes = raw.as_bytes();
    if bytes.len() == 1 {
        let b = bytes[0];
        if b.is_ascii() && b != b'"' && b != b'\n' && b != b'\r' {
            return Ok(b);
        }
    }
    match raw.trim().to_ascii_lowercase().as_str() {
        "tab" | "\\t" => Ok(b'\t'),
        "comma" => Ok(b','),
        "semicolon" => Ok(b';'),
        "pipe" | "bar" => Ok(b'|'),
        "space" => Ok(b' '),
        _ => Err(DataError::Schema(format!(
            "CSV source: '{DELIMITER_OPTION}' must be one ASCII character, or one of \
             tab / comma / semicolon / pipe / space; got '{raw}'"
        ))),
    }
}

/// The workbook extension of `path`, if it has one.
fn spreadsheet_extension(path: &str) -> Option<&'static str> {
    let ext = Path::new(path).extension()?.to_string_lossy().to_ascii_lowercase();
    SPREADSHEET_EXTENSIONS.into_iter().find(|e| *e == ext)
}

/// What one probe pass established.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Counts {
    /// Non-empty records, header included.
    records: usize,
    /// Fields in the header record (1 when the file has no separator).
    header_fields: usize,
}

/// Count records and header fields in one buffered byte pass.
///
/// Tracks quote depth so a field containing an embedded newline or delimiter is
/// counted as one field of one record, and skips blank lines the way the `csv`
/// reader does. Never allocates a field and never decodes UTF-8.
fn probe_counts(path: &Path, delimiter: u8) -> Result<Counts> {
    let mut reader = BufReader::with_capacity(64 * 1024, File::open(path)?);

    let mut in_quotes = false;
    let mut record_has_bytes = false;
    let mut in_header = true;
    let mut records = 0usize;
    let mut header_fields = 1usize;

    loop {
        let chunk = reader.fill_buf()?;
        if chunk.is_empty() {
            break;
        }
        let consumed = chunk.len();
        for &b in chunk {
            match b {
                // A doubled quote inside a quoted field toggles out then back
                // in, which nets to the correct depth.
                b'"' => {
                    in_quotes = !in_quotes;
                    record_has_bytes = true;
                }
                b'\n' if !in_quotes => {
                    if record_has_bytes {
                        records += 1;
                        in_header = false;
                    }
                    record_has_bytes = false;
                }
                b'\r' if !in_quotes => {}
                _ if b == delimiter && !in_quotes => {
                    if in_header {
                        header_fields += 1;
                    }
                    record_has_bytes = true;
                }
                _ => record_has_bytes = true,
            }
        }
        reader.consume(consumed);
    }
    // A final record with no trailing newline still counts.
    if record_has_bytes {
        records += 1;
    }

    Ok(Counts {
        records,
        header_fields: if records == 0 { 0 } else { header_fields },
    })
}

/// Whether the file holds at least one record that is not pure whitespace.
///
/// Reads at most a handful of leading lines, so an empty or whitespace-only
/// file is reported as such without opening the parser.
fn has_header_row(path: &Path) -> Result<bool> {
    let mut reader = BufReader::new(File::open(path)?);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(false);
        }
        if line.iter().any(|b| !b.is_ascii_whitespace()) {
            return Ok(true);
        }
    }
}

#[cfg(feature = "import")]
fn read_frame(path: &Path, delimiter: u8) -> Result<Frame> {
    let reader = BufReader::new(File::open(path)?);
    if delimiter == DEFAULT_DELIMITER {
        return crate::import::frame_from_csv(reader);
    }

    // Re-emit the records as canonical comma-delimited bytes rather than
    // duplicating the dtype / unit inference: `import::frame_from_csv` stays
    // the single place in this crate that decides what a column is. The writer
    // re-quotes anything that needs it, so a semicolon file whose fields
    // contain commas survives the hop intact.
    let mut rdr = ::csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .has_headers(false)
        .from_reader(reader);
    let mut wtr = ::csv::WriterBuilder::new().from_writer(Vec::<u8>::new());
    for rec in rdr.records() {
        let rec = rec.map_err(|e| DataError::Schema(format!("csv row: {e}")))?;
        wtr.write_record(rec.iter())
            .map_err(|e| DataError::Schema(format!("csv re-encode: {e}")))?;
    }
    let bytes = wtr
        .into_inner()
        .map_err(|e| DataError::Schema(format!("csv re-encode: {e}")))?;
    crate::import::frame_from_csv(bytes.as_slice())
}

#[cfg(not(feature = "import"))]
fn read_frame(_path: &Path, _delimiter: u8) -> Result<Frame> {
    Err(DataError::Schema(
        "CSV source: eustress-data was compiled without the `import` feature, so no CSV \
         parser is linked in. Enable `eustress-data/import` to fetch CSV files."
            .into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory that deletes itself, so the suite leaves nothing
    /// behind and two tests never collide on a filename.
    struct TempDir {
        path: std::path::PathBuf,
    }

    impl TempDir {
        fn new(tag: &str) -> Self {
            use std::sync::atomic::{AtomicUsize, Ordering};
            static SEQ: AtomicUsize = AtomicUsize::new(0);
            let n = SEQ.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir()
                .join("eustress_data_csv_test")
                .join(format!("{tag}_{}_{n}", std::process::id()));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn write(&self, name: &str, contents: &str) -> std::path::PathBuf {
            let p = self.path.join(name);
            std::fs::write(&p, contents).unwrap();
            p
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn source_at(path: &Path) -> CsvSource {
        CsvSource::new(SourceConfig::new(SourceKind::Csv, path.to_string_lossy()))
    }

    fn source_with(path: &Path, key: &str, value: &str) -> CsvSource {
        CsvSource::new(
            SourceConfig::new(SourceKind::Csv, path.to_string_lossy()).with_option(key, value),
        )
    }

    const SAMPLE: &str = "t (s),count,ok,label\n0.0,10,true,a\n0.5,20,false,b\n1.0,30,true,c\n";

    /// The checked-in fixture the crate's shared source harness blesses
    /// (`tests/fixtures/readings.csv`): header with unit symbols, four rows, one
    /// missing number and one missing trailing string. Reading it here keeps
    /// this provider honest against the same bytes every other provider is
    /// measured on. Checked in, so the test stays hermetic.
    fn shared_fixture() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/readings.csv")
    }

    // ── validation ───────────────────────────────────────────────────────────

    #[test]
    fn a_plain_path_validates_and_reports_the_csv_kind() {
        let c = SourceConfig::new(SourceKind::Csv, "data/readings.csv");
        assert!(validate(&c).is_ok());
        assert_eq!(CsvSource::new(c).kind(), SourceKind::Csv);
    }

    #[test]
    fn a_config_for_another_provider_is_refused() {
        let c = SourceConfig::new(SourceKind::Rest, "https://api.example.com/x");
        assert!(validate(&c).is_err(), "CsvSource must not accept a REST config");
    }

    #[test]
    fn the_shared_endpoint_rules_still_apply() {
        // Delegated to `super::validate_config`; asserted here so a future
        // change that bypasses it is caught.
        assert!(validate(&SourceConfig::new(SourceKind::Csv, "https://example.com/a.csv")).is_err());
        assert!(validate(&SourceConfig::new(SourceKind::Csv, "   ")).is_err());
    }

    #[test]
    fn every_workbook_extension_is_refused_with_an_actionable_message() {
        for ext in SPREADSHEET_EXTENSIONS {
            let c = SourceConfig::new(SourceKind::Csv, format!("books/quarterly.{ext}"));
            let err = validate(&c).unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("Excel workbook"), "{ext}: {msg}");
            assert!(msg.contains("Export the sheet to CSV"), "{ext}: {msg}");
        }
        // Case is not significant on the way in.
        assert!(validate(&SourceConfig::new(SourceKind::Csv, "Q3.XLSX")).is_err());
    }

    #[test]
    fn a_workbook_is_refused_by_probe_and_fetch_not_only_by_validate() {
        let src = CsvSource::new(SourceConfig::new(SourceKind::Csv, "books/quarterly.xlsx"));
        assert!(src.test_connection().is_err(), "probe must refuse a workbook");
        assert!(src.fetch().is_err(), "fetch must refuse a workbook");
    }

    #[test]
    fn delimiter_accepts_literals_and_names_and_rejects_the_rest() {
        assert_eq!(parse_delimiter(";").unwrap(), b';');
        assert_eq!(parse_delimiter("\t").unwrap(), b'\t');
        assert_eq!(parse_delimiter(" ").unwrap(), b' ', "a deliberate space must survive");
        assert_eq!(parse_delimiter("\\t").unwrap(), b'\t');
        assert_eq!(parse_delimiter("TAB").unwrap(), b'\t');
        assert_eq!(parse_delimiter("semicolon").unwrap(), b';');
        assert_eq!(parse_delimiter("pipe").unwrap(), b'|');

        for bad in ["", "::", "\"", "\n", "→"] {
            assert!(parse_delimiter(bad).is_err(), "accepted {bad:?} as a delimiter");
        }
    }

    #[test]
    fn an_absent_delimiter_option_means_comma() {
        let c = SourceConfig::new(SourceKind::Csv, "a.csv");
        assert_eq!(delimiter_of(&c).unwrap(), DEFAULT_DELIMITER);
        assert_eq!(delimiter_of(&c.with_option(DELIMITER_OPTION, ";")).unwrap(), b';');
    }

    #[test]
    fn a_bad_delimiter_fails_validation_not_the_read() {
        let c = SourceConfig::new(SourceKind::Csv, "a.csv").with_option(DELIMITER_OPTION, "::");
        assert!(validate(&c).is_err());
    }

    // ── probe ────────────────────────────────────────────────────────────────

    #[test]
    fn probe_reports_row_and_column_counts() {
        let dir = TempDir::new("probe_ok");
        let src = source_at(&dir.write("readings.csv", SAMPLE));
        let status = src.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);
        assert_eq!(status.detail, "3 data rows, 4 columns");
    }

    #[test]
    fn probe_reports_a_missing_file_as_unreachable() {
        let dir = TempDir::new("probe_missing");
        let src = source_at(&dir.path.join("absent.csv"));
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("no file at"), "{}", status.detail);
    }

    #[test]
    fn probe_reports_an_empty_file_as_unreachable() {
        let dir = TempDir::new("probe_empty");
        let src = source_at(&dir.write("empty.csv", ""));
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("is empty"), "{}", status.detail);
    }

    #[test]
    fn probe_reports_a_directory_as_unreachable() {
        let dir = TempDir::new("probe_dir");
        let src = source_at(&dir.path);
        let status = src.test_connection().unwrap();
        assert!(!status.reachable);
        assert!(status.detail.contains("is a directory"), "{}", status.detail);
    }

    #[test]
    fn probe_calls_out_a_header_only_file() {
        let dir = TempDir::new("probe_header_only");
        let src = source_at(&dir.write("header.csv", "t (s),count,label\n"));
        let status = src.test_connection().unwrap();
        assert!(status.reachable, "a schema with no rows is still readable");
        assert_eq!(status.detail, "0 data rows, 3 columns (header only)");
    }

    #[test]
    fn probe_honours_a_custom_delimiter() {
        let dir = TempDir::new("probe_semi");
        let path = dir.write("euro.csv", "a;b;c\n1;2;3\n4;5;6\n");
        // With the default comma the header looks like a single field.
        assert_eq!(source_at(&path).test_connection().unwrap().detail, "2 data rows, 1 columns");
        assert_eq!(
            source_with(&path, DELIMITER_OPTION, ";").test_connection().unwrap().detail,
            "2 data rows, 3 columns"
        );
    }

    #[test]
    fn probe_does_not_miscount_quoted_newlines_delimiters_or_blank_lines() {
        // The discriminating case: a naive newline count says 4 data rows and a
        // naive comma count says 3 header fields. Both are wrong.
        let dir = TempDir::new("probe_quoted");
        let src = source_at(&dir.write(
            "notes.csv",
            "name,note\r\na,\"line one\nline two\"\r\n\r\nb,\"has, a comma\"\r\n",
        ));
        let status = src.test_connection().unwrap();
        assert!(status.reachable, "{}", status.detail);
        assert_eq!(status.detail, "2 data rows, 2 columns");
    }

    #[test]
    fn probe_counts_a_final_record_with_no_trailing_newline() {
        let dir = TempDir::new("probe_no_eol");
        let src = source_at(&dir.write("tail.csv", "a,b\n1,2"));
        assert_eq!(src.test_connection().unwrap().detail, "1 data rows, 2 columns");
    }

    #[test]
    fn probe_agrees_with_the_shared_harness_fixture() {
        let path = shared_fixture();
        assert!(path.is_file(), "missing shared fixture at {}", path.display());
        assert_eq!(source_at(&path).test_connection().unwrap().detail, "4 data rows, 4 columns");
    }

    // ── fetch ────────────────────────────────────────────────────────────────

    #[cfg(feature = "import")]
    mod fetch {
        use super::*;
        use crate::{ColumnData, ColumnDtype};

        #[test]
        fn fetch_reads_values_dtypes_and_units() {
            let dir = TempDir::new("fetch_ok");
            let frame = source_at(&dir.write("readings.csv", SAMPLE)).fetch().unwrap();
            assert_eq!(frame.n_rows(), 3);
            assert_eq!(frame.n_cols(), 4);

            let t = frame.specs().find(|s| s.name == "t").unwrap();
            assert_eq!(t.dtype, ColumnDtype::F64);
            assert_eq!(t.unit.as_deref(), Some("s"), "unit parsed from 't (s)'");
            assert_eq!(
                frame.specs().find(|s| s.name == "count").unwrap().dtype,
                ColumnDtype::I64
            );
            match frame.column("label").unwrap() {
                ColumnData::Str(v) => assert_eq!(v[2].as_deref(), Some("c")),
                other => panic!("label should be Str, got {other:?}"),
            }
        }

        #[test]
        fn fetch_names_the_missing_file() {
            let dir = TempDir::new("fetch_missing");
            let err = source_at(&dir.path.join("absent.csv")).fetch().unwrap_err();
            assert!(err.to_string().contains("absent.csv"), "{err}");
        }

        #[test]
        fn fetch_rejects_an_empty_file_by_name() {
            let dir = TempDir::new("fetch_empty");
            let err = source_at(&dir.write("empty.csv", "")).fetch().unwrap_err();
            let msg = err.to_string();
            assert!(msg.contains("is empty"), "{msg}");
            assert!(msg.contains("header row"), "{msg}");

            // Whitespace only is empty too, and must not reach the parser.
            let err = source_at(&dir.write("blank.csv", "\n\n  \n")).fetch().unwrap_err();
            assert!(err.to_string().contains("is empty"), "{err}");
        }

        #[test]
        fn a_header_only_file_yields_a_zero_row_frame_with_its_schema() {
            // Zero-row frames are first class in this crate (see the Parquet
            // round-trip tests), so a header with no data is a schema, not a
            // failure. The empty-file case above is the one that errors.
            let dir = TempDir::new("fetch_header_only");
            let frame = source_at(&dir.write("header.csv", "t (s),count,label\n")).fetch().unwrap();
            assert_eq!(frame.n_rows(), 0);
            assert_eq!(frame.n_cols(), 3);
            let names: Vec<&str> = frame.specs().map(|s| s.name.as_str()).collect();
            assert_eq!(names, ["t", "count", "label"]);
            assert_eq!(
                frame.specs().find(|s| s.name == "t").unwrap().unit.as_deref(),
                Some("s"),
                "unit survives a row-less header"
            );
        }

        #[test]
        fn fetch_rejects_a_directory() {
            let dir = TempDir::new("fetch_dir");
            let err = source_at(&dir.path).fetch().unwrap_err();
            assert!(err.to_string().contains("is a directory"), "{err}");
        }

        #[test]
        fn a_custom_delimiter_yields_the_same_frame_as_its_comma_twin() {
            let dir = TempDir::new("fetch_semi");
            let comma = source_at(&dir.write("comma.csv", SAMPLE)).fetch().unwrap();
            let semi_text = SAMPLE.replace(',', ";");
            let semi = source_with(&dir.write("semi.csv", &semi_text), DELIMITER_OPTION, ";")
                .fetch()
                .unwrap();
            assert_eq!(semi, comma, "delimiter should change parsing, not the result");
        }

        #[test]
        fn a_tab_delimited_file_reads_through_the_named_alias() {
            let dir = TempDir::new("fetch_tab");
            let path = dir.write("readings.tsv", &SAMPLE.replace(',', "\t"));
            let frame = source_with(&path, DELIMITER_OPTION, "\\t").fetch().unwrap();
            assert_eq!(frame.n_rows(), 3);
            assert_eq!(frame.n_cols(), 4);
            assert_eq!(
                frame.specs().find(|s| s.name == "count").unwrap().dtype,
                ColumnDtype::I64
            );
        }

        #[test]
        fn a_custom_delimiter_preserves_commas_inside_fields() {
            // The re-encode hop is the risk here: a semicolon file whose fields
            // contain commas must not gain columns on the way through.
            let dir = TempDir::new("fetch_semi_commas");
            let path = dir.write("notes.csv", "name;note\na;has, a comma\nb;plain\n");
            let frame = source_with(&path, DELIMITER_OPTION, ";").fetch().unwrap();
            assert_eq!(frame.n_cols(), 2);
            assert_eq!(frame.n_rows(), 2);
            match frame.column("note").unwrap() {
                ColumnData::Str(v) => assert_eq!(v[0].as_deref(), Some("has, a comma")),
                other => panic!("note should be Str, got {other:?}"),
            }
        }

        #[test]
        fn the_shared_harness_fixture_reads_with_its_units_and_gaps() {
            let frame = source_at(&shared_fixture()).fetch().unwrap();
            assert_eq!(frame.n_rows(), 4);
            assert_eq!(frame.n_cols(), 4);

            let pressure = frame.specs().find(|s| s.name == "pressure").unwrap();
            assert_eq!(pressure.dtype, ColumnDtype::F64);
            assert_eq!(pressure.unit.as_deref(), Some("psi"));
            assert_eq!(
                frame.specs().find(|s| s.name == "time").unwrap().unit.as_deref(),
                Some("s")
            );
            assert_eq!(
                frame.specs().find(|s| s.name == "valve_open").unwrap().dtype,
                ColumnDtype::Bool
            );

            // The two deliberate gaps must survive as nulls, not as zero or "".
            match frame.column("pressure").unwrap() {
                ColumnData::F64(v) => assert_eq!(v[2], None, "the missing number must be null"),
                other => panic!("pressure should be F64, got {other:?}"),
            }
            match frame.column("operator").unwrap() {
                ColumnData::Str(v) => {
                    assert_eq!(v[3], None, "the missing trailing string must be null");
                    assert_eq!(v[0].as_deref(), Some("alvarez"));
                }
                other => panic!("operator should be Str, got {other:?}"),
            }
        }

        #[test]
        fn a_quoted_field_keeps_its_embedded_newline() {
            let dir = TempDir::new("fetch_quoted");
            let path = dir.write("notes.csv", "name,note\na,\"line one\nline two\"\nb,plain\n");
            let frame = source_at(&path).fetch().unwrap();
            assert_eq!(frame.n_rows(), 2, "an embedded newline is not a row break");
            match frame.column("note").unwrap() {
                ColumnData::Str(v) => assert_eq!(v[0].as_deref(), Some("line one\nline two")),
                other => panic!("note should be Str, got {other:?}"),
            }
        }
    }

    /// Without the `import` feature there is no parser linked in, so `fetch`
    /// must say so rather than pretending the file is malformed.
    #[cfg(not(feature = "import"))]
    #[test]
    fn fetch_without_the_import_feature_names_the_missing_feature() {
        let dir = TempDir::new("fetch_no_feature");
        let err = source_at(&dir.write("readings.csv", SAMPLE)).fetch().unwrap_err();
        assert!(err.to_string().contains("import"), "{err}");
    }
}
