#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum MarketId {
    Cn,
    Hk,
}

impl MarketId {
    pub(crate) fn parse(value: &str) -> Option<Self> {
        match value {
            "cn" => Some(Self::Cn),
            "hk" => Some(Self::Hk),
            _ => None,
        }
    }

    fn wire_value(self) -> &'static str {
        match self {
            Self::Cn => "cn",
            Self::Hk => "hk",
        }
    }

    fn canonical_code_length(self) -> usize {
        match self {
            Self::Cn => 6,
            Self::Hk => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SecurityKind {
    OrdinaryShare,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
pub(crate) struct SecurityId {
    market: MarketId,
    code: String,
    kind: SecurityKind,
}

impl SecurityId {
    pub(crate) fn new(market: MarketId, code: &str, kind: SecurityKind) -> Result<Self, String> {
        if code.len() != market.canonical_code_length()
            || !code.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(format!(
                "{} security code must contain exactly {} ASCII digits",
                market.wire_value(),
                market.canonical_code_length()
            ));
        }

        Ok(Self {
            market,
            code: code.to_owned(),
            kind,
        })
    }

    pub(crate) fn market(&self) -> MarketId {
        self.market
    }

    pub(crate) fn code(&self) -> &str {
        &self.code
    }

    pub(crate) fn kind(&self) -> SecurityKind {
        self.kind
    }

    pub(crate) fn key(&self) -> String {
        format!(
            "{}:{}:{}",
            self.market.wire_value(),
            match self.kind {
                SecurityKind::OrdinaryShare => "ordinary_share",
            },
            self.code
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize)]
#[serde(transparent)]
pub(crate) struct CurrencyCode(String);

impl CurrencyCode {
    pub(crate) fn new(value: &str) -> Result<Self, String> {
        if value.len() != 3 || !value.bytes().all(|byte| byte.is_ascii_alphabetic()) {
            return Err("currency code must contain exactly three ASCII letters".to_owned());
        }

        Ok(Self(value.to_ascii_uppercase()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Money {
    amount: f64,
    currency: CurrencyCode,
}

impl Money {
    pub(crate) fn new(amount: f64, currency: CurrencyCode) -> Result<Self, String> {
        if !amount.is_finite() {
            return Err("money amount must be finite".to_owned());
        }

        Ok(Self { amount, currency })
    }

    pub(crate) fn amount(&self) -> f64 {
        self.amount
    }
}

impl serde::Serialize for Money {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if !self.amount.is_finite() {
            return Err(serde::ser::Error::custom("money amount must be finite"));
        }

        let mut value = serializer.serialize_struct("Money", 2)?;
        serde::ser::SerializeStruct::serialize_field(&mut value, "amount", &self.amount)?;
        serde::ser::SerializeStruct::serialize_field(&mut value, "currency", &self.currency)?;
        serde::ser::SerializeStruct::end(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Quantity {
    amount: f64,
    unit: String,
}

impl Quantity {
    pub(crate) fn new(amount: f64, unit: impl Into<String>) -> Result<Self, String> {
        if !amount.is_finite() {
            return Err("quantity amount must be finite".to_owned());
        }

        let unit = unit.into();
        if unit.trim().is_empty() {
            return Err("quantity unit must not be blank".to_owned());
        }

        Ok(Self { amount, unit })
    }

    pub(crate) fn unit(&self) -> &str {
        &self.unit
    }
}

impl serde::Serialize for Quantity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if !self.amount.is_finite() {
            return Err(serde::ser::Error::custom("quantity amount must be finite"));
        }

        let mut value = serializer.serialize_struct("Quantity", 2)?;
        serde::ser::SerializeStruct::serialize_field(&mut value, "amount", &self.amount)?;
        serde::ser::SerializeStruct::serialize_field(&mut value, "unit", &self.unit)?;
        serde::ser::SerializeStruct::end(value)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct ObservationMetadata {
    pub(crate) source_id: String,
    pub(crate) fetched_at: Option<String>,
    pub(crate) observed_at: Option<String>,
    pub(crate) disclosed_at: Option<String>,
    pub(crate) report_period: Option<String>,
    pub(crate) verification_status: String,
    pub(crate) quality_flags: Vec<String>,
    pub(crate) missing_reason: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct CoverageSummary {
    pub(crate) requested_count: u64,
    pub(crate) available_count: u64,
    pub(crate) missing_count: u64,
    pub(crate) start_date: Option<String>,
    pub(crate) end_date: Option<String>,
    pub(crate) missing_fields: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityId {
    SecurityDirectory,
    Observe,
    FinancialSnapshot,
    Screening,
    Backtest,
    NewsResearch,
    Sentiment,
    ResearchAgent,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CapabilityStatus {
    Available,
    Limited,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct MarketCapability {
    pub(crate) market: MarketId,
    pub(crate) id: CapabilityId,
    pub(crate) status: CapabilityStatus,
    pub(crate) reason: Option<String>,
    pub(crate) coverage: Option<CoverageSummary>,
}

#[cfg(test)]
mod tests {
    use super::{
        CapabilityId, CapabilityStatus, CoverageSummary, CurrencyCode, MarketCapability, MarketId,
        Money, ObservationMetadata, Quantity, SecurityId, SecurityKind,
    };

    #[test]
    fn money_and_quantity_keep_explicit_units_and_reject_nonfinite_values() {
        let hkd = CurrencyCode::new("hkd").unwrap();
        assert_eq!(hkd.as_str(), "HKD");
        assert_eq!(Money::new(12.5, hkd).unwrap().amount(), 12.5);
        assert!(CurrencyCode::new("HK").is_err());
        assert!(CurrencyCode::new("H1D").is_err());
        assert!(CurrencyCode::new("ＨＫＤ").is_err());
        assert!(Money::new(f64::NAN, CurrencyCode::new("HKD").unwrap()).is_err());
        assert!(Money::new(f64::INFINITY, CurrencyCode::new("HKD").unwrap()).is_err());
        assert_eq!(
            Money::new(-12.5, CurrencyCode::new("HKD").unwrap())
                .unwrap()
                .amount(),
            -12.5
        );
        assert_eq!(Quantity::new(100.0, "share").unwrap().unit(), "share");
        assert!(Quantity::new(f64::NEG_INFINITY, "share").is_err());
        assert!(Quantity::new(1.0, "  ").is_err());
    }

    #[test]
    fn money_quantity_and_market_metadata_serialize_with_explicit_wire_fields() {
        let money = Money::new(12.5, CurrencyCode::new("hkd").unwrap()).unwrap();
        let quantity = Quantity::new(100.0, "share").unwrap();

        assert_eq!(
            serde_json::to_value(money).unwrap(),
            serde_json::json!({"amount": 12.5, "currency": "HKD"})
        );
        assert_eq!(
            serde_json::to_value(quantity).unwrap(),
            serde_json::json!({"amount": 100.0, "unit": "share"})
        );

        let metadata = ObservationMetadata {
            source_id: "hkex".to_owned(),
            fetched_at: Some("2026-10-22T00:00:00Z".to_owned()),
            observed_at: None,
            disclosed_at: None,
            report_period: None,
            verification_status: "unverified".to_owned(),
            quality_flags: Vec::new(),
            missing_reason: None,
        };
        assert_eq!(metadata.observed_at, None);
        assert_eq!(metadata.disclosed_at, None);
        assert_eq!(metadata.quality_flags, Vec::<String>::new());
        assert_eq!(
            serde_json::to_value(metadata).unwrap(),
            serde_json::json!({
                "source_id": "hkex",
                "fetched_at": "2026-10-22T00:00:00Z",
                "observed_at": null,
                "disclosed_at": null,
                "report_period": null,
                "verification_status": "unverified",
                "quality_flags": [],
                "missing_reason": null
            })
        );

        let coverage = CoverageSummary {
            requested_count: 2,
            available_count: 1,
            missing_count: 1,
            start_date: Some("2026-10-16".to_owned()),
            end_date: Some("2026-10-17".to_owned()),
            missing_fields: vec!["daily_history".to_owned()],
        };
        assert_eq!(
            serde_json::to_value(coverage).unwrap(),
            serde_json::json!({
                "requested_count": 2,
                "available_count": 1,
                "missing_count": 1,
                "start_date": "2026-10-16",
                "end_date": "2026-10-17",
                "missing_fields": ["daily_history"]
            })
        );
    }

    #[test]
    fn market_id_uses_stable_wire_values() {
        assert_eq!(serde_json::to_string(&MarketId::Hk).unwrap(), "\"hk\"");
        assert_eq!(serde_json::to_string(&MarketId::Cn).unwrap(), "\"cn\"");
        assert_eq!(MarketId::parse("hk"), Some(MarketId::Hk));
        assert_eq!(MarketId::parse("cn"), Some(MarketId::Cn));
        assert_eq!(MarketId::parse("HK"), None);
        assert_eq!(MarketId::parse("hk "), None);
    }

    #[test]
    fn security_id_preserves_hk_leading_zeroes_and_rejects_noncanonical_code() {
        let id = SecurityId::new(MarketId::Hk, "00700", SecurityKind::OrdinaryShare).unwrap();

        assert_eq!(id.market(), MarketId::Hk);
        assert_eq!(id.code(), "00700");
        assert_eq!(id.kind(), SecurityKind::OrdinaryShare);
        assert_eq!(id.key(), "hk:ordinary_share:00700");
        assert_eq!(
            serde_json::to_value(&id).unwrap(),
            serde_json::json!({
                "market": "hk",
                "code": "00700",
                "kind": "ordinary_share"
            })
        );

        assert!(SecurityId::new(MarketId::Hk, "700", SecurityKind::OrdinaryShare).is_err());
        assert!(SecurityId::new(MarketId::Hk, "00700.HK", SecurityKind::OrdinaryShare).is_err());
    }

    #[test]
    fn security_id_requires_six_ascii_digits_for_china_codes() {
        assert!(SecurityId::new(MarketId::Cn, "600000", SecurityKind::OrdinaryShare).is_ok());
        assert!(SecurityId::new(MarketId::Cn, "60000", SecurityKind::OrdinaryShare).is_err());
        assert!(
            SecurityId::new(MarketId::Cn, "６０００００", SecurityKind::OrdinaryShare).is_err()
        );
    }

    #[test]
    fn market_contract_serialization_matches_shared_fixture() {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../testdata/market-domain-contract.json")).unwrap();

        let mut sample = serialize_market_contract_sample();
        assert_eq!(
            sample["quantity"]["amount"].as_f64(),
            fixture["quantity"]["amount"].as_f64(),
            "JSON integer and float spellings represent the same quantity value"
        );
        sample["quantity"]["amount"] = fixture["quantity"]["amount"].clone();
        assert_eq!(sample, fixture);
    }

    fn serialize_market_contract_sample() -> serde_json::Value {
        let security = SecurityId::new(MarketId::Hk, "00700", SecurityKind::OrdinaryShare).unwrap();
        let money = Money::new(12.5, CurrencyCode::new("HKD").unwrap()).unwrap();
        let quantity = Quantity::new(100.0, "share").unwrap();
        let capability = MarketCapability {
            market: MarketId::Hk,
            id: CapabilityId::Observe,
            status: CapabilityStatus::Limited,
            reason: Some("daily history is missing".to_owned()),
            coverage: Some(CoverageSummary {
                requested_count: 2,
                available_count: 1,
                missing_count: 1,
                start_date: Some("2026-10-16".to_owned()),
                end_date: Some("2026-10-17".to_owned()),
                missing_fields: vec!["daily_history".to_owned()],
            }),
        };

        serde_json::json!({
            "market": "hk",
            "security": security,
            "money": money,
            "quantity": quantity,
            "capability": capability,
        })
    }
}
