use serde::Serialize;

pub const REPORT_VERSION: u32 = 1;
pub const METHOD: &str = "residue-inspection/1";

/// Labels the live-control inspection must see, or the method is not sensitive enough to trust.
pub const LIVE_REQUIRED: [&str; 6] = [
    "certificate_der",
    "pairing_token",
    "pairing_token_hex",
    "private_key_limbs",
    "qr_payload",
    "tls_fingerprint",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Expectation {
    Found,
    Clean,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Pass,
    Fail,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScopeResult {
    pub scope: &'static str,
    pub method: &'static str,
    pub swept: bool,
    pub findings: Vec<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stores: Vec<&'static str>,
    /// Memory only: writable regions the kernel refused to read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable_regions: Option<u64>,
    /// Credential stores only: a planted secret in a scratch home was found by the same sweep.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_detected: Option<bool>,
    /// Credential stores only: stores that exist but could not be read.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable_stores: Option<u64>,
    /// Scopes that are not swept at run time: what asserts them instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub basis: Option<&'static str>,
}

impl ScopeResult {
    pub fn new(scope: &'static str, method: &'static str, swept: bool) -> Self {
        Self {
            scope,
            method,
            swept,
            findings: Vec::new(),
            files: None,
            stores: Vec::new(),
            unreadable_regions: None,
            control_detected: None,
            unreadable_stores: None,
            basis: None,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Inspection {
    pub case: &'static str,
    pub expectation: Expectation,
    pub scopes: Vec<ScopeResult>,
    pub verdict: Verdict,
}

impl Inspection {
    pub fn judge(case: &'static str, expectation: Expectation, scopes: Vec<ScopeResult>) -> Self {
        let verdict = match expectation {
            Expectation::Found => live_control_verdict(&scopes),
            Expectation::Clean => clean_verdict(&scopes),
        };
        Self {
            case,
            expectation,
            scopes,
            verdict,
        }
    }
}

fn live_control_verdict(scopes: &[ScopeResult]) -> Verdict {
    let seen = scopes
        .iter()
        .find(|scope| scope.scope == "process_memory")
        .map(|scope| &scope.findings);
    let sensitive = seen.is_some_and(|seen| LIVE_REQUIRED.iter().all(|label| seen.contains(label)));
    if sensitive {
        Verdict::Pass
    } else {
        Verdict::Fail
    }
}

fn clean_verdict(scopes: &[ScopeResult]) -> Verdict {
    let clean = scopes.iter().all(|scope| {
        scope.findings.is_empty()
            && scope.files.is_none_or(|files| files == 0)
            && scope.unreadable_regions.is_none_or(|regions| regions == 0)
            && scope.unreadable_stores.is_none_or(|stores| stores == 0)
            && scope.control_detected.is_none_or(|detected| detected)
    });
    if clean { Verdict::Pass } else { Verdict::Fail }
}

#[derive(Debug, Serialize)]
pub struct Canary {
    pub memory_detected: bool,
    pub state_detected: bool,
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub passed: bool,
}

#[derive(Debug, Serialize)]
pub struct Report {
    pub report_version: u32,
    pub method: &'static str,
    pub platform: &'static str,
    /// C-02 asks for this same method on the reference devices; that run is not part of this report.
    pub device_run: &'static str,
    pub seed: u64,
    pub needles: Vec<&'static str>,
    pub inspections: Vec<Inspection>,
    pub canary: Canary,
    pub summary: Summary,
}

impl Report {
    pub fn new(
        seed: u64,
        needles: Vec<&'static str>,
        inspections: Vec<Inspection>,
        canary: Canary,
    ) -> Self {
        let passed = inspections
            .iter()
            .all(|inspection| inspection.verdict == Verdict::Pass)
            && canary.memory_detected
            && canary.state_detected;
        Self {
            report_version: REPORT_VERSION,
            method: METHOD,
            platform: "linux",
            device_run: "open",
            seed,
            needles,
            inspections,
            canary,
            summary: Summary { passed },
        }
    }
}
