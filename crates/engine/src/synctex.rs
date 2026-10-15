//! Bidirectional source ↔ PDF lookup through the system `synctex` CLI
//! (ships with TeX Live and MiKTeX). The `.synctex.gz` file records
//! absolute source paths, so callers must pass the source path as it was
//! handed to the engine (we compile with a relative name — resolve it
//! against the document directory first).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A highlighted region in the PDF, in PDF points with the origin at the
/// top-left of the page.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfLocation {
    pub page: u32,
    pub h: f64,
    pub v: f64,
    pub width: f64,
    pub height: f64,
}

/// A position in a `.tex` source file.
#[derive(Debug, Clone, PartialEq)]
pub struct SourceLocation {
    pub file: PathBuf,
    pub line: u32,
    pub column: Option<u32>,
}

/// The `synctex` binary is on PATH.
pub fn is_available() -> bool {
    Command::new("synctex").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

/// Forward search: source position → PDF boxes.
pub fn forward(pdf: &Path, source: &Path, line: u32, column: u32) -> Result<Vec<PdfLocation>, String> {
    let input = format!("{line}:{column}:{}", source.display());
    let out = run(&["view", "-i", &input, "-o", &pdf.to_string_lossy()], pdf)?;
    Ok(parse_view(&out))
}

/// Inverse search: point on a PDF page (PDF points, top-left origin) →
/// source position.
pub fn inverse(pdf: &Path, page: u32, x: f64, y: f64) -> Result<Option<SourceLocation>, String> {
    let output = format!("{page}:{x:.2}:{y:.2}:{}", pdf.display());
    let out = run(&["edit", "-o", &output], pdf)?;
    Ok(parse_edit(&out))
}

fn run(args: &[&str], pdf: &Path) -> Result<String, String> {
    let workdir = pdf.parent().unwrap_or_else(|| Path::new("."));
    let output =
        Command::new("synctex").args(args).current_dir(workdir).stdin(Stdio::null()).output().map_err(|e| format!("failed to run synctex: {e}"))?;
    if !output.status.success() {
        return Err(format!("synctex failed: {}", String::from_utf8_lossy(&output.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn parse_view(out: &str) -> Vec<PdfLocation> {
    let mut results = Vec::new();
    let mut current: Option<PdfLocation> = None;
    for line in out.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let num = || value.trim().parse::<f64>().ok();
        match key {
            "Page" => {
                if let Some(loc) = current.take() {
                    results.push(loc);
                }
                current = value.trim().parse().ok().map(|page| PdfLocation { page, h: 0.0, v: 0.0, width: 0.0, height: 0.0 });
            }
            "h" => {
                if let (Some(c), Some(n)) = (current.as_mut(), num()) {
                    c.h = n;
                }
            }
            "v" => {
                if let (Some(c), Some(n)) = (current.as_mut(), num()) {
                    c.v = n;
                }
            }
            "W" => {
                if let (Some(c), Some(n)) = (current.as_mut(), num()) {
                    c.width = n;
                }
            }
            "H" => {
                if let (Some(c), Some(n)) = (current.as_mut(), num()) {
                    c.height = n;
                }
            }
            _ => {}
        }
    }
    results.extend(current);
    results
}

fn parse_edit(out: &str) -> Option<SourceLocation> {
    let mut file = None;
    let mut line = None;
    let mut column = None;
    for l in out.lines() {
        if let Some(v) = l.strip_prefix("Input:") {
            file.get_or_insert_with(|| PathBuf::from(v.trim()));
        } else if let Some(v) = l.strip_prefix("Line:") {
            line = line.or_else(|| v.trim().parse().ok());
        } else if let Some(v) = l.strip_prefix("Column:") {
            column = column.or_else(|| v.trim().parse::<i64>().ok().and_then(|c| u32::try_from(c).ok()));
        }
    }
    Some(SourceLocation { file: file?, line: line?, column })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_view_output() {
        let out = "SyncTeX result begin\nOutput:/p/main.pdf\nPage:1\nx:148.0\ny:136.1\nh:133.77\nv:139.08\nW:343.71\nH:9.96\nbefore:\noffset:0\nmiddle:\nafter:\nSyncTeX result end\n";
        assert_eq!(parse_view(out), vec![PdfLocation { page: 1, h: 133.77, v: 139.08, width: 343.71, height: 9.96 }]);
    }

    #[test]
    fn parses_edit_output() {
        let out = "SyncTeX result begin\nOutput:/p/main.pdf\nInput:/p/./main.tex\nLine:4\nColumn:-1\nOffset:0\nContext:\nSyncTeX result end\n";
        let loc = parse_edit(out).unwrap_or_else(|| panic!("expected a location"));
        assert_eq!(loc.line, 4);
        assert_eq!(loc.column, None);
        assert_eq!(loc.file, PathBuf::from("/p/./main.tex"));
    }
}
