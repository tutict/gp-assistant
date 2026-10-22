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

#[cfg(test)]
mod tests {
    use super::{MarketId, SecurityId, SecurityKind};

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
}
