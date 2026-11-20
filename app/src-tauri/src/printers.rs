//! Printer discovery and job submission (TASK-08-005, PLAN §2J Print).
//!
//! The page rasters come from the render crate's layout engine; this module
//! only hands the finished document to the operating system's spooler:
//! CUPS (`lpstat`/`lpoptions`/`lp`) on macOS and Linux, the .NET print stack
//! through PowerShell on Windows. Nothing here renders or touches originals.
//! Every external program is run with an argument vector (never a shell
//! string), and only printer names the spooler itself listed are submitted.

use std::path::{Path, PathBuf};
use std::process::Command;

use fotobase_render::print::PaperSize;

/// An installed printer and the paper sizes it is known to take.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Printer {
    pub name: String,
    pub is_default: bool,
    /// Letter/A4 sizes the printer lists as supported.
    pub papers: Vec<PaperSize>,
    /// The paper currently loaded/selected by default, when it is Letter or A4.
    pub default_paper: Option<PaperSize>,
    /// Paper sizes of the print dialog this printer does not match; printing
    /// on one of them warns first.
    pub mismatch: Vec<PaperSize>,
}

impl Printer {
    pub fn new(
        name: String,
        is_default: bool,
        papers: Vec<PaperSize>,
        default_paper: Option<PaperSize>,
    ) -> Self {
        let mismatch = [PaperSize::Letter, PaperSize::A4]
            .into_iter()
            .filter(|p| paper_mismatch(&papers, default_paper, *p))
            .collect();
        Self { name, is_default, papers, default_paper, mismatch }
    }
}

/// Whether printing `job` on a printer would not match its paper: it lists
/// supported sizes and the job's is not among them, or — when it lists none
/// — its default paper differs. Unknown paper data never warns.
pub fn paper_mismatch(
    supported: &[PaperSize],
    default_paper: Option<PaperSize>,
    job: PaperSize,
) -> bool {
    if !supported.is_empty() {
        !supported.contains(&job)
    } else {
        default_paper.is_some_and(|d| d != job)
    }
}

/// Map a spooler paper name (`Letter`, `A4`, `na_letter_8.5x11in`,
/// `iso_a4_210x297mm`, `A4.Fullbleed`) to a supported paper size.
pub fn parse_paper_name(name: &str) -> Option<PaperSize> {
    let lower = name.trim().to_ascii_lowercase();
    let base = lower.split('.').next().unwrap_or(&lower);
    match base {
        "letter" | "na_letter_8.5x11in" | "8.5x11in" | "usletter" | "us letter" => {
            Some(PaperSize::Letter)
        }
        "a4" | "iso_a4_210x297mm" | "210x297mm" | "a4 (210 x 297 mm)" => Some(PaperSize::A4),
        _ if lower.starts_with("na_letter_") => Some(PaperSize::Letter),
        _ if lower.starts_with("iso_a4_") => Some(PaperSize::A4),
        _ if lower.starts_with("letter ") || lower.starts_with("letter(") => {
            Some(PaperSize::Letter)
        }
        _ if lower.starts_with("a4 ") || lower.starts_with("a4(") => Some(PaperSize::A4),
        _ => None,
    }
}

/// What the spooler takes as the job's document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentKind {
    /// One PDF holding every sheet.
    Pdf,
    /// One PNG per sheet.
    Pages,
}

#[derive(Debug, Clone, Copy)]
pub enum Document<'a> {
    Pdf(&'a Path),
    Pages(&'a [PathBuf]),
}

/// Result of a submitted job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submission {
    /// Spooler job id, when it reports one (CUPS: `HP_LaserJet-12`).
    pub job_id: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SpoolError {
    #[error("could not run {program}: {source}")]
    Spawn { program: String, source: std::io::Error },
    #[error("{program} failed: {message}")]
    Failed { program: String, message: String },
    #[error("unreadable printer list: {0}")]
    Parse(String),
    #[error("unknown printer: {0}")]
    UnknownPrinter(String),
    #[error("document does not match what the spooler takes")]
    WrongDocument,
}

/// The operating system's print spooler.
pub trait Spooler: Send + Sync {
    /// Which document form `submit` expects.
    fn kind(&self) -> DocumentKind;
    fn printers(&self) -> Result<Vec<Printer>, SpoolError>;
    /// Hand `document` to `printer`; only names from [`Spooler::printers`]
    /// are accepted.
    fn submit(
        &self,
        printer: &str,
        paper: PaperSize,
        title: &str,
        document: Document<'_>,
    ) -> Result<Submission, SpoolError>;
}

/// The spooler of the running OS.
pub fn system_spooler() -> Box<dyn Spooler> {
    #[cfg(windows)]
    {
        Box::new(PowerShell::default())
    }
    #[cfg(not(windows))]
    {
        Box::new(Cups::default())
    }
}

fn run(program: &Path, args: &[&std::ffi::OsStr]) -> Result<std::process::Output, SpoolError> {
    Command::new(program)
        .args(args)
        .output()
        .map_err(|source| SpoolError::Spawn { program: program.display().to_string(), source })
}

fn require_ok(program: &Path, out: &std::process::Output) -> Result<(), SpoolError> {
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let message = [stderr.trim(), stdout.trim()]
        .into_iter()
        .find(|m| !m.is_empty())
        .unwrap_or("non-zero exit status")
        .to_owned();
    Err(SpoolError::Failed { program: program.display().to_string(), message })
}

// -- CUPS (macOS, Linux) -----------------------------------------------------

/// `lpstat -e` output: one destination per line.
pub fn parse_lpstat_destinations(out: &str) -> Vec<String> {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_owned).collect()
}

/// `lpstat -d` output: `system default destination: NAME`.
pub fn parse_lpstat_default(out: &str) -> Option<String> {
    out.lines()
        .find_map(|l| l.split_once("default destination:"))
        .map(|(_, name)| name.trim().to_owned())
        .filter(|n| !n.is_empty())
}

/// `lpoptions -p NAME -l` output: the `PageSize` line lists the media
/// choices, the current default marked with `*`.
pub fn parse_lpoptions_paper(out: &str) -> (Vec<PaperSize>, Option<PaperSize>) {
    let mut papers = Vec::new();
    let mut default = None;
    let Some(line) = out.lines().find(|l| l.starts_with("PageSize/") || l.starts_with("media/"))
    else {
        return (papers, default);
    };
    let choices = line.split_once(':').map_or("", |(_, c)| c);
    for choice in choices.split_whitespace() {
        let (is_default, name) = match choice.strip_prefix('*') {
            Some(n) => (true, n),
            None => (false, choice),
        };
        let Some(paper) = parse_paper_name(name) else { continue };
        if !papers.contains(&paper) {
            papers.push(paper);
        }
        if is_default {
            default = Some(paper);
        }
    }
    (papers, default)
}

/// `lp` output: `request id is NAME-12 (1 file(s))`.
pub fn parse_lp_job_id(out: &str) -> Option<String> {
    let rest = out.split_once("request id is ")?.1;
    rest.split_whitespace().next().map(str::to_owned)
}

fn cups_media(paper: PaperSize) -> &'static str {
    match paper {
        PaperSize::Letter => "Letter",
        PaperSize::A4 => "A4",
    }
}

#[cfg(unix)]
pub use cups::Cups;

#[cfg(unix)]
mod cups {
    use super::*;
    use std::ffi::OsStr;

    /// CUPS command-line spooler; the programs are looked up on `PATH`
    /// unless a directory is given (tests point it at a fake spooler).
    pub struct Cups {
        lpstat: PathBuf,
        lpoptions: PathBuf,
        lp: PathBuf,
    }

    impl Default for Cups {
        fn default() -> Self {
            Self { lpstat: "lpstat".into(), lpoptions: "lpoptions".into(), lp: "lp".into() }
        }
    }

    impl Cups {
        /// Use the `lpstat`, `lpoptions` and `lp` programs found in `dir`.
        pub fn in_dir(dir: &Path) -> Self {
            Self {
                lpstat: dir.join("lpstat"),
                lpoptions: dir.join("lpoptions"),
                lp: dir.join("lp"),
            }
        }
    }

    impl Spooler for Cups {
        fn kind(&self) -> DocumentKind {
            DocumentKind::Pdf
        }

        fn printers(&self) -> Result<Vec<Printer>, SpoolError> {
            let listed = run(&self.lpstat, &[OsStr::new("-e")])?;
            // CUPS exits non-zero with nothing on stdout when no printer exists.
            if !listed.status.success() && listed.stdout.is_empty() {
                return Ok(Vec::new());
            }
            require_ok(&self.lpstat, &listed)?;
            let default = run(&self.lpstat, &[OsStr::new("-d")])
                .ok()
                .and_then(|o| parse_lpstat_default(&String::from_utf8_lossy(&o.stdout)));
            let mut printers = Vec::new();
            for name in parse_lpstat_destinations(&String::from_utf8_lossy(&listed.stdout)) {
                let opts =
                    run(&self.lpoptions, &[OsStr::new("-p"), OsStr::new(&name), OsStr::new("-l")])
                        .ok()
                        .filter(|o| o.status.success());
                let (papers, default_paper) = opts
                    .map(|o| parse_lpoptions_paper(&String::from_utf8_lossy(&o.stdout)))
                    .unwrap_or_default();
                let is_default = default.as_deref() == Some(name.as_str());
                printers.push(Printer::new(name, is_default, papers, default_paper));
            }
            Ok(printers)
        }

        fn submit(
            &self,
            printer: &str,
            paper: PaperSize,
            title: &str,
            document: Document<'_>,
        ) -> Result<Submission, SpoolError> {
            let Document::Pdf(file) = document else { return Err(SpoolError::WrongDocument) };
            if !self.printers()?.iter().any(|p| p.name == printer) {
                return Err(SpoolError::UnknownPrinter(printer.to_owned()));
            }
            let media = format!("media={}", cups_media(paper));
            let out = run(
                &self.lp,
                &[
                    OsStr::new("-d"),
                    OsStr::new(printer),
                    OsStr::new("-t"),
                    OsStr::new(title),
                    OsStr::new("-n"),
                    OsStr::new("1"),
                    OsStr::new("-o"),
                    OsStr::new(&media),
                    OsStr::new("-o"),
                    OsStr::new("fit-to-page"),
                    file.as_os_str(),
                ],
            )?;
            require_ok(&self.lp, &out)?;
            Ok(Submission { job_id: parse_lp_job_id(&String::from_utf8_lossy(&out.stdout)) })
        }
    }
}

// -- Windows -----------------------------------------------------------------

/// Output of `Get-CimInstance Win32_Printer | Select-Object Name,Default,
/// PrinterPaperNames | ConvertTo-Json`: an array, or a bare object for a
/// single printer.
pub fn parse_windows_printers(json: &str) -> Result<Vec<Printer>, SpoolError> {
    let json = json.trim().trim_start_matches('\u{feff}');
    if json.is_empty() {
        return Ok(Vec::new());
    }
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| SpoolError::Parse(e.to_string()))?;
    let entries = match value {
        serde_json::Value::Array(a) => a,
        other => vec![other],
    };
    let mut printers = Vec::new();
    for entry in entries {
        let Some(name) = entry.get("Name").and_then(|n| n.as_str()) else { continue };
        let is_default = entry.get("Default").and_then(|d| d.as_bool()).unwrap_or(false);
        let mut papers = Vec::new();
        let names = match entry.get("PrinterPaperNames") {
            Some(serde_json::Value::Array(a)) => a.clone(),
            Some(v @ serde_json::Value::String(_)) => vec![v.clone()],
            _ => Vec::new(),
        };
        for paper in names.iter().filter_map(|n| n.as_str()).filter_map(parse_paper_name) {
            if !papers.contains(&paper) {
                papers.push(paper);
            }
        }
        printers.push(Printer::new(name.to_owned(), is_default, papers, None));
    }
    Ok(printers)
}

#[cfg(windows)]
pub use windows::PowerShell;

#[cfg(windows)]
mod windows {
    use super::*;
    use std::ffi::OsStr;

    /// Prints the page PNGs with `System.Drawing.Printing.PrintDocument`
    /// driven from PowerShell: no PDF handler needs to be installed.
    pub struct PowerShell {
        program: PathBuf,
    }

    impl Default for PowerShell {
        fn default() -> Self {
            Self { program: "powershell".into() }
        }
    }

    const LIST_SCRIPT: &str = "Get-CimInstance Win32_Printer | \
        Select-Object Name,Default,PrinterPaperNames | ConvertTo-Json -Compress";

    // Pages are full-sheet rasters: each is drawn over the whole page, with
    // the printer's hard margin compensated so the layout margins hold.
    const PRINT_SCRIPT: &str = r#"
param([string]$Printer, [string]$Paper, [string]$Title, [Parameter(ValueFromRemainingArguments)][string[]]$Files)
Add-Type -AssemblyName System.Drawing
$doc = New-Object System.Drawing.Printing.PrintDocument
$doc.PrinterSettings.PrinterName = $Printer
if (-not $doc.PrinterSettings.IsValid) { [Console]::Error.WriteLine("unknown printer"); exit 2 }
$doc.DocumentName = $Title
$doc.PrintController = New-Object System.Drawing.Printing.StandardPrintController
$doc.DefaultPageSettings.Margins = New-Object System.Drawing.Printing.Margins(0, 0, 0, 0)
foreach ($size in $doc.PrinterSettings.PaperSizes) {
  if ($size.PaperName -match $Paper) { $doc.DefaultPageSettings.PaperSize = $size; break }
}
$script:files = $Files
$script:index = 0
$doc.add_PrintPage({
  param($sender, $e)
  $image = [System.Drawing.Image]::FromFile($script:files[$script:index])
  try {
    $e.Graphics.TranslateTransform(-$e.PageSettings.HardMarginX, -$e.PageSettings.HardMarginY)
    $e.Graphics.DrawImage($image, 0, 0, $e.PageBounds.Width, $e.PageBounds.Height)
  } finally { $image.Dispose() }
  $script:index++
  $e.HasMorePages = $script:index -lt $script:files.Count
})
$doc.Print()
"#;

    impl PowerShell {
        fn base_args() -> Vec<&'static OsStr> {
            ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass"]
                .into_iter()
                .map(OsStr::new)
                .collect()
        }
    }

    impl Spooler for PowerShell {
        fn kind(&self) -> DocumentKind {
            DocumentKind::Pages
        }

        fn printers(&self) -> Result<Vec<Printer>, SpoolError> {
            let mut args = Self::base_args();
            args.extend([OsStr::new("-Command"), OsStr::new(LIST_SCRIPT)]);
            let out = run(&self.program, &args)?;
            require_ok(&self.program, &out)?;
            parse_windows_printers(&String::from_utf8_lossy(&out.stdout))
        }

        fn submit(
            &self,
            printer: &str,
            paper: PaperSize,
            title: &str,
            document: Document<'_>,
        ) -> Result<Submission, SpoolError> {
            let Document::Pages(files) = document else { return Err(SpoolError::WrongDocument) };
            if !self.printers()?.iter().any(|p| p.name == printer) {
                return Err(SpoolError::UnknownPrinter(printer.to_owned()));
            }
            let script = files
                .first()
                .and_then(|f| f.parent())
                .ok_or_else(|| SpoolError::Parse("no pages".into()))?
                .join("print.ps1");
            std::fs::write(&script, PRINT_SCRIPT)
                .map_err(|source| SpoolError::Spawn { program: "print.ps1".into(), source })?;
            let mut args = Self::base_args();
            let paper = cups_media(paper);
            args.extend([
                OsStr::new("-File"),
                script.as_os_str(),
                OsStr::new("-Printer"),
                OsStr::new(printer),
                OsStr::new("-Paper"),
                OsStr::new(paper),
                OsStr::new("-Title"),
                OsStr::new(title),
            ]);
            args.extend(files.iter().map(|f| f.as_os_str()));
            let out = run(&self.program, &args)?;
            require_ok(&self.program, &out)?;
            Ok(Submission { job_id: None })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paper_names_map_to_letter_and_a4() {
        for n in ["Letter", "letter", "na_letter_8.5x11in", "Letter.Fullbleed", "US Letter"] {
            assert_eq!(parse_paper_name(n), Some(PaperSize::Letter), "{n}");
        }
        for n in ["A4", "iso_a4_210x297mm", "A4.Fullbleed", "A4 (210 x 297 mm)"] {
            assert_eq!(parse_paper_name(n), Some(PaperSize::A4), "{n}");
        }
        for n in ["Legal", "4x6", "A5", "Letterhead", "Custom.WIDTHxHEIGHT"] {
            assert_eq!(parse_paper_name(n), None, "{n}");
        }
    }

    #[test]
    fn mismatch_uses_supported_sizes_then_the_default() {
        use PaperSize::{Letter, A4};
        assert!(paper_mismatch(&[A4], Some(A4), Letter), "letter not offered");
        assert!(!paper_mismatch(&[A4, Letter], Some(A4), Letter), "offered, so chosen via media");
        assert!(paper_mismatch(&[], Some(A4), Letter), "no list: default decides");
        assert!(!paper_mismatch(&[], Some(Letter), Letter));
        assert!(!paper_mismatch(&[], None, Letter), "unknown never warns");
        let p = Printer::new("p".into(), false, vec![A4], Some(A4));
        assert_eq!(p.mismatch, vec![Letter]);
    }

    #[test]
    fn parses_cups_output() {
        assert_eq!(parse_lpstat_destinations("A\n\nB_2\n"), vec!["A", "B_2"]);
        assert_eq!(
            parse_lpstat_default("system default destination: HP_M402\n").as_deref(),
            Some("HP_M402")
        );
        assert_eq!(parse_lpstat_default("no system default destination\n"), None);
        assert_eq!(
            parse_lp_job_id("request id is HP_M402-12 (1 file(s))\n").as_deref(),
            Some("HP_M402-12")
        );
        assert_eq!(parse_lp_job_id("oops"), None);

        let opts = "Duplex/2-Sided: *None DuplexNoTumble\n\
            PageSize/Media Size: 4x6 *A4 Legal Letter Letter.Fullbleed Custom.WIDTHxHEIGHT\n";
        assert_eq!(
            parse_lpoptions_paper(opts),
            (vec![PaperSize::A4, PaperSize::Letter], Some(PaperSize::A4))
        );
        assert_eq!(parse_lpoptions_paper("Duplex/x: *None"), (vec![], None));
    }

    #[test]
    fn parses_windows_printer_json() {
        let many = r#"[{"Name":"Brother HL","Default":true,"PrinterPaperNames":["A4","Letter","Legal"]},
            {"Name":"PDF Writer","Default":false,"PrinterPaperNames":null}]"#;
        let printers = parse_windows_printers(many).unwrap();
        assert_eq!(printers.len(), 2);
        assert!(printers[0].is_default);
        assert_eq!(printers[0].papers, vec![PaperSize::A4, PaperSize::Letter]);
        assert!(printers[1].papers.is_empty() && printers[1].mismatch.is_empty());

        // PowerShell emits a bare object for a single printer.
        let one = r#"{"Name":"Solo","Default":false,"PrinterPaperNames":["Letter"]}"#;
        let printers = parse_windows_printers(one).unwrap();
        assert_eq!(printers[0].mismatch, vec![PaperSize::A4]);
        assert!(parse_windows_printers("").unwrap().is_empty());
        assert!(parse_windows_printers("{nope").is_err());
    }
}
