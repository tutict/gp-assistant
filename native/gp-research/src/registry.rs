use crate::*;
use gp_core::ScoringParameters;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResolvedParameters {
    pub parameters: ScoringParameters,
    pub dataset_id: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    #[test]
    fn freeze_is_idempotent_atomic_and_reverification_keeps_signature() {
        let path = std::env::temp_dir().join(format!(
            "gp-freeze-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = ResearchStore::open(&path).unwrap();
        let record = store
            .import_bundle(
                &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny"),
            )
            .unwrap();
        let date = |v| NaiveDate::parse_from_str(v, "%Y-%m-%d").unwrap();
        let split = CalibrationSplit {
            train_start: date("2016-01-01"),
            train_end: date("2020-09-30"),
            validation_start: date("2020-10-01"),
            validation_end: date("2021-09-30"),
            test_start: date("2021-10-01"),
            test_end: date("2026-09-30"),
            train_last_signal: date("2020-08-17"),
            validation_last_signal: date("2021-08-17"),
            purge_sessions: 31,
            training_months: 57,
            validation_months: 12,
            test_months: 60,
            warmup_bars: 120,
        };
        let mut p = gp_core::fixed_scoring_parameters();
        p.algorithm_version = "adaptive_swing_v2".into();
        p.parameter_set_id = "origin-a".into();
        store
            .freeze_test_parameters(&record.dataset_id, &p, &split)
            .unwrap();
        store
            .freeze_test_parameters(&record.dataset_id, &p, &split)
            .unwrap();
        let mut reverification = p.clone();
        reverification.parameter_set_id = "origin-current-code".into();
        store
            .freeze_test_parameters(&record.dataset_id, &reverification, &split)
            .unwrap();
        assert_eq!(
            weight_signature(&p).unwrap(),
            weight_signature(&reverification).unwrap()
        );
        let mut changed = p.clone();
        let m = changed.modes.get_mut("range").unwrap();
        *m.weights.get_mut("quality").unwrap() += 0.01;
        *m.weights.get_mut("valuation").unwrap() -= 0.01;
        assert_eq!(
            store
                .freeze_test_parameters(&record.dataset_id, &changed, &split)
                .unwrap_err()
                .code,
            "test_locked"
        );
        let raw: String = store
            .conn
            .query_row("SELECT parameters FROM test_locks", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            serde_json::from_str::<ScoringParameters>(&raw)
                .unwrap()
                .parameter_set_id,
            "origin-a"
        );
        let mut different_split = split.clone();
        different_split.train_start = date("2017-01-01");
        assert_eq!(
            store
                .freeze_test_parameters(&record.dataset_id, &p, &different_split)
                .unwrap_err()
                .code,
            "test_locked"
        );
        assert_eq!(
            store
                .resolve_parameters(None)
                .unwrap()
                .parameters
                .parameter_set_id,
            "fixed-v1"
        );
        let reader = ResearchStore::open_readonly(&path).unwrap();
        assert_eq!(
            reader
                .freeze_test_parameters(&record.dataset_id, &p, &split)
                .unwrap_err()
                .code,
            "readonly"
        );
    }
}
pub(crate) fn check_registry_version(conn: &Connection) -> ResearchResult<()> {
    let version: u32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    let identity: u32 = conn.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    if version > 2 || (version == 2 && identity != 1196446258) {
        return Err(ResearchError::new(
            "newer_schema",
            "newer or foreign registry schema preserved",
        ));
    }
    if version < 2 && identity != 0 {
        return Err(ResearchError::new(
            "foreign_schema",
            "foreign registry identity preserved",
        ));
    }
    // Match the actual SQLite catalog against the single creation contract.
    // Names/columns alone miss PK, NOT NULL, CHECK, collation, hidden columns
    // and foreign triggers. Compare SQL tokens: spacing/case/IF NOT EXISTS
    // can be equivalent, but joined identifiers/types are different tokens;
    // every other schema change needs an explicitly supported migration.
    fn canonical(sql: &str) -> Option<Vec<String>> {
        let mut chars = sql.chars().peekable();
        let mut tokens = Vec::new();
        let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '$') || !c.is_ascii();
        while let Some(c) = chars.next() {
            if c.is_ascii_whitespace() {
                continue;
            }
            if c == '-' && chars.peek() == Some(&'-') {
                chars.next();
                for next in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                continue;
            }
            if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                let mut closed = false;
                while let Some(next) = chars.next() {
                    if next == '*' && chars.peek() == Some(&'/') {
                        chars.next();
                        closed = true;
                        break;
                    }
                }
                if !closed {
                    return None;
                }
                continue;
            }
            if matches!(c, '\'' | '"' | '`' | '[') {
                // Quoted tokens retain spelling/content, including escaped
                // delimiters. Never lowercase literals or erase their spaces.
                let end = if c == '[' { ']' } else { c };
                let mut token = c.to_string();
                let mut closed = false;
                while let Some(next) = chars.next() {
                    token.push(next);
                    if next == end {
                        if chars.peek() == Some(&end) {
                            token.push(chars.next().unwrap());
                        } else {
                            closed = true;
                            break;
                        }
                    }
                }
                if !closed {
                    return None;
                }
                tokens.push(token);
                continue;
            }
            if word(c) {
                let mut token = c.to_string();
                while chars.peek().is_some_and(|next| word(*next)) {
                    token.push(chars.next().unwrap());
                }
                tokens.push(token.to_ascii_lowercase());
                continue;
            }
            let mut token = c.to_string();
            if chars.peek().is_some_and(|next| {
                matches!(
                    (c, *next),
                    ('=', '=')
                        | ('<', '=')
                        | ('>', '=')
                        | ('!', '=')
                        | ('<', '>')
                        | ('|', '|')
                        | ('-', '>')
                )
            }) {
                token.push(chars.next().unwrap());
                if token == "->" && chars.peek() == Some(&'>') {
                    token.push(chars.next().unwrap());
                }
            }
            tokens.push(token);
        }
        if tokens
            .iter()
            .take(5)
            .map(String::as_str)
            .eq(["create", "table", "if", "not", "exists"])
        {
            tokens.drain(2..5);
        }
        Some(tokens)
    }
    let expected: std::collections::BTreeMap<&str, Vec<String>> = crate::dataset::REGISTRY_SCHEMA
        .split(';')
        .filter_map(|sql| {
            let declaration = sql.trim().strip_prefix("CREATE TABLE IF NOT EXISTS ")?;
            let name = declaration.split_once('(')?.0;
            if version == 2 || version == 1 && matches!(name, "datasets" | "reports") {
                Some((name, canonical(sql)?))
            } else {
                None
            }
        })
        .collect();
    let mut stmt =
        conn.prepare("SELECT name,type,sql FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'")?;
    let objects = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if objects.len() != expected.len()
        || objects.iter().any(|(name, kind, sql)| {
            kind != "table"
                || sql.as_ref().is_none_or(|sql| {
                    expected
                        .get(name.as_str())
                        .is_none_or(|expected| canonical(sql).as_ref() != Some(expected))
                })
        })
    {
        return Err(ResearchError::new("foreign_schema","unrecognized/partial registry tables or constraints; preserved before migration/mutation"));
    }
    Ok(())
}
fn version(conn: &Connection) -> ResearchResult<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}
pub(crate) fn parameter_digest(p: &ScoringParameters) -> ResearchResult<String> {
    Ok(crate::import::digest(&serde_json::to_vec(p)?))
}
pub(crate) fn weight_signature(p: &ScoringParameters) -> ResearchResult<String> {
    Ok(crate::import::digest(&serde_json::to_vec(&p.modes)?))
}
fn candidate(conn: &Connection, id: &str) -> ResearchResult<ResolvedParameters> {
    if version(conn)? < 2 {
        return Err(ResearchError::new(
            "not_found",
            "candidate registry unavailable",
        ));
    }
    let (payload, dataset, core, research, fingerprint): (String, String, String, String, String) =
        conn.query_row(
            "SELECT payload,dataset_id,core,research,fingerprint FROM parameter_sets WHERE id=?",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()?
        .ok_or_else(|| ResearchError::new("not_found", "parameter set not registered"))?;
    let p: ScoringParameters = serde_json::from_str(&payload)?;
    gp_core::validate_scoring_parameters(&p)?;
    if p.parameter_set_id != id
        || p.algorithm_version != "adaptive_swing_v2"
        || core != gp_core::scoring_implementation_fingerprint()
        || research != research_implementation_fingerprint()
        || fingerprint != parameter_digest(&p)?
    {
        return Err(ResearchError::new(
            "stale_parameters",
            "candidate fingerprint or identity mismatch",
        ));
    }
    Ok(ResolvedParameters {
        parameters: p,
        dataset_id: Some(dataset),
    })
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct CalibrationReceipt {
    pub report_digest: String,
    pub core: String,
    pub research: String,
    pub data: String,
    pub parameter: String,
    pub signature: String,
    pub split: Option<CalibrationSplit>,
}
impl ResearchStore {
    pub(crate) fn freeze_test_parameters(
        &self,
        dataset_id: &str,
        p: &ScoringParameters,
        split: &CalibrationSplit,
    ) -> ResearchResult<()> {
        self.require_write()?;
        gp_core::validate_scoring_parameters(p)?;
        self.dataset(dataset_id)?;
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| {
            let signature = weight_signature(p)?;
            let old:Option<(String,String)>=self.conn.query_row("SELECT signature,split FROM test_locks WHERE dataset_id=? AND test_start=? AND test_end=?",params![dataset_id,split.test_start.to_string(),split.test_end.to_string()],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let serialized_split = serde_json::to_string(split)?;
            if old.is_some_and(|(old, old_split)| old != signature || old_split != serialized_split)
            {
                return Err(ResearchError::new(
                    "test_locked",
                    "different weights/split cannot reopen this dataset/test window",
                ));
            }
            self.conn.execute(
                "INSERT OR IGNORE INTO test_locks VALUES(?,?,?,?,?,?)",
                params![
                    dataset_id,
                    split.test_start.to_string(),
                    split.test_end.to_string(),
                    signature,
                    serde_json::to_string(p)?,
                    serialized_split
                ],
            )?;
            Ok(())
        })();
        match result {
            Ok(()) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(())
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK;")?;
                Err(e)
            }
        }
    }
    pub fn register_parameters(
        &self,
        p: &ScoringParameters,
        dataset_id: &str,
    ) -> ResearchResult<()> {
        self.require_write()?;
        gp_core::validate_scoring_parameters(p)?;
        if p.algorithm_version != "adaptive_swing_v2" {
            return Err(ResearchError::new(
                "parameters",
                "only immutable v2 candidates can be registered",
            ));
        }
        self.dataset(dataset_id)?;
        let payload = serde_json::to_string(p)?;
        let fingerprint = parameter_digest(p)?;
        let existing:Option<(String,String,String,String,String)>=self.conn.query_row("SELECT payload,dataset_id,core,research,fingerprint FROM parameter_sets WHERE id=?",[&p.parameter_set_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
        let expected = (
            payload.clone(),
            dataset_id.to_owned(),
            gp_core::scoring_implementation_fingerprint().to_owned(),
            research_implementation_fingerprint().to_owned(),
            fingerprint.clone(),
        );
        if let Some(old) = existing {
            if old != expected {
                return Err(ResearchError::new(
                    "immutable_conflict",
                    "parameter id has a different origin or immutable evidence",
                ));
            }
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO parameter_sets VALUES(?,?,?,?,?,?)",
            params![
                p.parameter_set_id,
                payload,
                dataset_id,
                expected.2,
                expected.3,
                fingerprint
            ],
        )?;
        Ok(())
    }
    pub fn research_parameters(&self, id: &str) -> ResearchResult<ResolvedParameters> {
        if id == "fixed-v1" {
            return Ok(ResolvedParameters {
                parameters: gp_core::fixed_scoring_parameters(),
                dataset_id: None,
            });
        }
        let p = candidate(&self.registry_reader()?, id)?;
        self.dataset(p.dataset_id.as_deref().expect("candidate origin"))?;
        Ok(p)
    }
    pub fn resolve_parameters(
        &self,
        requested: Option<&str>,
    ) -> ResearchResult<ResolvedParameters> {
        if requested == Some("fixed-v1") {
            return self.research_parameters("fixed-v1");
        }
        let conn = self.registry_reader()?;
        let id = match requested {
            Some(id) => id.to_owned(),
            None => {
                if version(&conn)? < 2 {
                    return self.research_parameters("fixed-v1");
                }
                match conn
                    .query_row(
                        "SELECT parameter_id FROM active_parameters WHERE singleton=1",
                        [],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?
                {
                    Some(id) => id,
                    None => return self.research_parameters("fixed-v1"),
                }
            }
        };
        if id == "fixed-v1" {
            return self.research_parameters(&id);
        }
        let run: Option<String> = conn
            .query_row(
                "SELECT run_id FROM qualifications WHERE parameter_id=?",
                [&id],
                |r| r.get(0),
            )
            .optional()?;
        let run = run.ok_or_else(|| {
            ResearchError::new("eligibility", "candidate has no verified qualification")
        })?;
        let verified = self.verify_receipt(&conn, &run)?;
        if verified.parameters.parameter_set_id != id {
            return Err(ResearchError::new(
                "eligibility",
                "qualification points to a different parameter set",
            ));
        }
        Ok(verified)
    }
    fn verify_receipt(&self, conn: &Connection, run: &str) -> ResearchResult<ResolvedParameters> {
        if version(conn)? < 2 {
            return Err(ResearchError::new(
                "eligibility",
                "calibration receipt unavailable",
            ));
        }
        let receipt: String = conn
            .query_row(
                "SELECT payload FROM calibration_receipts WHERE run_id=?",
                [run],
                |r| r.get(0),
            )
            .optional()?
            .ok_or_else(|| {
                ResearchError::new(
                    "eligibility",
                    "no calibration-owned receipt; generic reports cannot promote",
                )
            })?;
        let receipt: CalibrationReceipt = serde_json::from_str(&receipt)?;
        let raw: String =
            conn.query_row("SELECT report FROM reports WHERE run_id=?", [run], |r| {
                r.get(0)
            })?;
        let value: serde_json::Value = serde_json::from_str(&raw)?;
        let report: CalibrationReport = serde_json::from_value(value.clone())?;
        if report.status != "complete" || report.parameters.algorithm_version != "adaptive_swing_v2"
        {
            return Err(ResearchError::new(
                "eligibility",
                "only a complete qualified v2 calibration can promote",
            ));
        }
        let p = candidate(conn, &report.parameters.parameter_set_id)?;
        let d = self.dataset(&report.dataset_id)?;
        let split = report
            .split
            .as_ref()
            .ok_or_else(|| ResearchError::new("eligibility", "split evidence missing"))?;
        let lock:Option<(String,String,String)>=conn.query_row("SELECT signature,parameters,split FROM test_locks WHERE dataset_id=? AND test_start=? AND test_end=?",params![report.dataset_id,split.test_start.to_string(),split.test_end.to_string()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
        let lock = lock.ok_or_else(|| ResearchError::new("eligibility", "test lock missing"))?;
        let gate = release_gate(&crate::calibration::gate_evidence(&report)?);
        let valid = report.run_id == run
            && report.status == "complete"
            && report.gate.passed
            && gate.passed
            && serde_json::to_value(&gate)? == serde_json::to_value(&report.gate)?
            && receipt.report_digest == crate::import::digest(raw.as_bytes())
            && receipt.core == gp_core::scoring_implementation_fingerprint()
            && receipt.research == research_implementation_fingerprint()
            && receipt.data == d.record().data_fingerprint
            && receipt.parameter == parameter_digest(&p.parameters)?
            && receipt.signature == weight_signature(&p.parameters)?
            && receipt.signature == lock.0
            && receipt.split.as_ref() == Some(split)
            && serde_json::to_string(split)? == lock.2
            && weight_signature(&serde_json::from_str::<ScoringParameters>(&lock.1)?)?
                == receipt.signature
            && p.dataset_id.as_deref() == Some(report.dataset_id.as_str())
            && parameter_digest(&report.parameters)? == receipt.parameter
            && d.record().real_data
            && d.record().full_universe
            && compute_split(d.calendar(), Some(split.test_end))? == *split;
        if !valid {
            return Err(ResearchError::new(
                "eligibility",
                "calibration evidence, split, digest or gates do not verify",
            ));
        }
        Ok(p)
    }
    pub fn qualification_verified(&self, run_id: &str) -> ResearchResult<bool> {
        let conn = self.registry_reader()?;
        match self.verify_receipt(&conn, run_id) {
            Ok(_) => Ok(true),
            Err(e)
                if matches!(
                    e.code.as_str(),
                    "eligibility"
                        | "not_found"
                        | "stale_parameters"
                        | "core"
                        | "json"
                        | "immutable_conflict"
                        | "insufficient_data"
                ) =>
            {
                Ok(false)
            }
            Err(e) => Err(e),
        }
    }
    pub fn promote(&self, run_id: &str) -> ResearchResult<ResolvedParameters> {
        self.require_write()?;
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| {
            let p = self
                .verify_receipt(&self.conn, run_id)
                .map_err(|e| ResearchError {
                    code: "eligibility".into(),
                    message: e.message,
                    details: e.details,
                })?;
            self.conn.execute("INSERT INTO qualifications VALUES(?,?) ON CONFLICT(parameter_id) DO UPDATE SET run_id=excluded.run_id",params![p.parameters.parameter_set_id,run_id])?;
            self.conn.execute("INSERT INTO active_parameters VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET parameter_id=excluded.parameter_id",[&p.parameters.parameter_set_id])?;
            Ok(p)
        })();
        match result {
            Ok(p) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(p)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK;")?;
                Err(e)
            }
        }
    }
    pub fn rollback(&self, id: &str) -> ResearchResult<ResolvedParameters> {
        self.require_write()?;
        self.conn.execute_batch("BEGIN IMMEDIATE;")?;
        let result = (|| {
            let p = if id == "fixed-v1" {
                ResolvedParameters {
                    parameters: gp_core::fixed_scoring_parameters(),
                    dataset_id: None,
                }
            } else {
                let run: String = self
                    .conn
                    .query_row(
                        "SELECT run_id FROM qualifications WHERE parameter_id=?",
                        [id],
                        |r| r.get(0),
                    )
                    .optional()?
                    .ok_or_else(|| {
                        ResearchError::new(
                            "eligibility",
                            "rollback requires previous qualification",
                        )
                    })?;
                let p = self.verify_receipt(&self.conn, &run)?;
                if p.parameters.parameter_set_id != id {
                    return Err(ResearchError::new(
                        "eligibility",
                        "rollback qualification id differs",
                    ));
                }
                p
            };
            self.conn.execute("INSERT INTO active_parameters VALUES(1,?) ON CONFLICT(singleton) DO UPDATE SET parameter_id=excluded.parameter_id",[id])?;
            Ok(p)
        })();
        match result {
            Ok(p) => {
                self.conn.execute_batch("COMMIT;")?;
                Ok(p)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK;")?;
                Err(e)
            }
        }
    }
    pub(crate) fn calibration_receipt(&self, report: &CalibrationReport) -> ResearchResult<()> {
        self.require_write()?;
        let split = report.split.clone();
        let raw: String = self.conn.query_row(
            "SELECT report FROM reports WHERE run_id=?",
            [&report.run_id],
            |r| r.get(0),
        )?;
        let receipt = CalibrationReceipt {
            report_digest: crate::import::digest(raw.as_bytes()),
            core: gp_core::scoring_implementation_fingerprint().into(),
            research: research_implementation_fingerprint().into(),
            data: self
                .dataset(&report.dataset_id)?
                .record()
                .data_fingerprint
                .clone(),
            parameter: parameter_digest(&report.parameters)?,
            signature: weight_signature(&report.parameters)?,
            split,
        };
        let payload = serde_json::to_string(&receipt)?;
        let old: Option<String> = self
            .conn
            .query_row(
                "SELECT payload FROM calibration_receipts WHERE run_id=?",
                [&report.run_id],
                |r| r.get(0),
            )
            .optional()?;
        if old.as_ref().is_some_and(|v| v != &payload) {
            return Err(ResearchError::new("immutable_conflict", "receipt differs"));
        }
        self.conn.execute(
            "INSERT OR IGNORE INTO calibration_receipts VALUES(?,?)",
            params![report.run_id, payload],
        )?;
        Ok(())
    }
}
