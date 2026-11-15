use crate::rates::ratio_percentage_points;
use crate::screening::classification::is_cold_sector;
use crate::screening::scoring::normalized_score_profile;
use crate::screening::types::{ScreenCriteria, ScreenResultGroup, ScreenedStock};
use std::collections::{HashMap, HashSet};

const SCREEN_GROUP_LIMIT: usize = 10;

pub(super) const POTENTIAL_SCORE_THRESHOLD: f64 = 10.0;

const THEME_PROMOTION_ORDER: [&str; 7] = [
    "materials",
    "ai_chain",
    "semiconductor_wafer",
    "tech",
    "energy",
    "medical",
    "game",
];

const THEME_FILL_ORDER: [&str; 7] = [
    "ai_chain",
    "semiconductor_wafer",
    "materials",
    "tech",
    "energy",
    "medical",
    "game",
];

fn should_promote_hot_sectors(criteria: &ScreenCriteria) -> bool {
    criteria.industry.as_deref().unwrap_or("").trim().is_empty()
        && criteria.sort_by.trim().eq_ignore_ascii_case("score")
        && !criteria.sort_dir.trim().eq_ignore_ascii_case("asc")
        && normalized_score_profile(&criteria.score_profile) != "quality"
}

fn promote_hot_sector_items(
    screened: &[ScreenedStock],
    criteria: &ScreenCriteria,
    limit: usize,
) -> Vec<ScreenedStock> {
    if !should_promote_hot_sectors(criteria) || limit == 0 {
        return screened.iter().take(limit).cloned().collect();
    }

    let mut promoted = Vec::new();
    let mut used_codes = HashSet::new();
    for category in THEME_PROMOTION_ORDER {
        if let Some(candidate) = screened.iter().find(|item| {
            !used_codes.contains(&item.stock.code)
                && item.theme_category.as_deref() == Some(category)
        }) {
            promoted.push(candidate.clone());
            used_codes.insert(candidate.stock.code.clone());
            if promoted.len() >= limit {
                return promoted;
            }
        }
    }

    for category in THEME_FILL_ORDER {
        for item in screened {
            if used_codes.contains(&item.stock.code)
                || item.theme_category.as_deref() != Some(category)
            {
                continue;
            }
            promoted.push(item.clone());
            used_codes.insert(item.stock.code.clone());
            if promoted.len() >= limit {
                return promoted;
            }
        }
    }

    for item in screened {
        if used_codes.contains(&item.stock.code) || is_cold_sector(&item.stock.industry) {
            continue;
        }
        promoted.push(item.clone());
        used_codes.insert(item.stock.code.clone());
        if promoted.len() >= limit {
            return promoted;
        }
    }

    for item in screened {
        if used_codes.contains(&item.stock.code) {
            continue;
        }
        promoted.push(item.clone());
        used_codes.insert(item.stock.code.clone());
        if promoted.len() >= limit {
            break;
        }
    }
    promoted
}

fn diversify_by_industry(screened: &[ScreenedStock], limit: usize) -> Vec<ScreenedStock> {
    if limit == 0 || screened.len() <= 3 {
        return screened.iter().take(limit).cloned().collect();
    }
    let per_industry_limit = ((limit as f64) * 0.45).ceil().max(2.0) as usize;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut selected = Vec::new();
    let mut deferred = Vec::new();

    for item in screened {
        let industry = if item.stock.industry.trim().is_empty() {
            "__unknown__".to_string()
        } else {
            item.stock.industry.trim().to_string()
        };
        let count = counts.get(&industry).copied().unwrap_or(0);
        if count < per_industry_limit {
            counts.insert(industry, count + 1);
            selected.push(item.clone());
        } else {
            deferred.push(item.clone());
        }
        if selected.len() >= limit {
            return selected;
        }
    }

    for item in deferred {
        selected.push(item);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

pub(super) fn sort_screened(screened: &mut [ScreenedStock], criteria: &ScreenCriteria) {
    let sort_by = criteria.sort_by.trim().to_ascii_lowercase();
    let ascending = criteria.sort_dir.trim().eq_ignore_ascii_case("asc");
    screened.sort_by(|left, right| {
        let ordering = match (sort_value(left, &sort_by), sort_value(right, &sort_by)) {
            (Some(left_value), Some(right_value)) => {
                if ascending {
                    left_value.total_cmp(&right_value)
                } else {
                    right_value.total_cmp(&left_value)
                }
            }
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        };
        ordering
            .then_with(|| right.score.total_cmp(&left.score))
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
}

fn should_diversify_by_industry(criteria: &ScreenCriteria) -> bool {
    criteria.industry.as_deref().unwrap_or("").trim().is_empty()
        && criteria.sort_by.trim().eq_ignore_ascii_case("score")
        && !criteria.sort_dir.trim().eq_ignore_ascii_case("asc")
        && normalized_score_profile(&criteria.score_profile) != "quality"
}

pub(super) fn primary_screen_items(
    screened: &[ScreenedStock],
    criteria: &ScreenCriteria,
    limit: usize,
) -> (Vec<ScreenedStock>, bool) {
    if !should_promote_hot_sectors(criteria) || limit == 0 {
        let items = if should_diversify_by_industry(criteria) {
            diversify_by_industry(screened, limit)
        } else {
            screened.iter().take(limit).cloned().collect()
        };
        let changed = items
            .iter()
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>()
            != screened
                .iter()
                .take(limit)
                .map(|item| item.stock.code.as_str())
                .collect::<Vec<_>>();
        return (items, changed);
    }
    let promoted = promote_hot_sector_items(screened, criteria, limit);
    let promoted_flag = promoted
        .iter()
        .map(|item| item.stock.code.as_str())
        .collect::<Vec<_>>()
        != screened
            .iter()
            .take(limit)
            .map(|item| item.stock.code.as_str())
            .collect::<Vec<_>>();
    (promoted, promoted_flag)
}

pub(super) fn screen_result_groups(screened: &[ScreenedStock]) -> Vec<ScreenResultGroup> {
    let hot_items = hot_group_items(screened, SCREEN_GROUP_LIMIT);
    let hot_codes: HashSet<String> = hot_items
        .iter()
        .map(|item| item.stock.code.clone())
        .collect();
    let potential_items = potential_group_items(
        screened,
        SCREEN_GROUP_LIMIT,
        &hot_codes,
        POTENTIAL_SCORE_THRESHOLD,
    );
    vec![
        ScreenResultGroup {
            key: "hot".to_string(),
            title: "\u{70ed}\u{95e8}\u{80a1}".to_string(),
            description: "AI\u{4e0a}\u{4e0b}\u{6e38}\u{3001}\u{82af}\u{7247}\u{3001}\u{7b97}\u{529b}\u{3001}\u{80fd}\u{6e90}\u{3001}\u{65b0}\u{6750}\u{6599}\u{3001}\u{533b}\u{836f}\u{3001}\u{6e38}\u{620f}\u{7b49}\u{70ed}\u{95e8}\u{65b9}\u{5411}\u{ff0c}\u{6309}\u{7efc}\u{5408}\u{5206}\u{548c}\u{4e3b}\u{9898}\u{4f18}\u{5148}\u{7ea7}\u{5c55}\u{793a}\u{3002}".to_string(),
            total: screened.iter().filter(|item| item.theme_category.is_some()).count(),
            returned: hot_items.len(),
            items: hot_items,
        },
        ScreenResultGroup {
            key: "potential".to_string(),
            title: "\u{6f5c}\u{529b}\u{80a1}".to_string(),
            description: format!("{}{}{}", "\u{7efc}\u{5408}\u{5206}\u{5927}\u{4e8e} ", POTENTIAL_SCORE_THRESHOLD, " \u{7684}\u{5019}\u{9009}\u{ff0c}\u{5df2}\u{5c3d}\u{91cf}\u{907f}\u{514d}\u{4e0e}\u{70ed}\u{95e8}\u{80a1}\u{91cd}\u{590d}\u{3002}"),
            total: screened.iter().filter(|item| item.score > POTENTIAL_SCORE_THRESHOLD).count(),
            returned: potential_items.len(),
            items: potential_items,
        },
    ]
}

fn hot_group_items(screened: &[ScreenedStock], limit: usize) -> Vec<ScreenedStock> {
    let mut selected = Vec::new();
    let mut used_codes = HashSet::new();
    let mut by_score = screened.to_vec();
    by_score.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
    for category in THEME_PROMOTION_ORDER {
        if let Some(candidate) = first_by_theme(&by_score, category, &used_codes) {
            append_selected(&mut selected, &mut used_codes, candidate);
            if selected.len() >= limit {
                return selected;
            }
        }
    }
    for category in THEME_FILL_ORDER {
        for item in &by_score {
            if used_codes.contains(&item.stock.code)
                || item.theme_category.as_deref() != Some(category)
            {
                continue;
            }
            append_selected(&mut selected, &mut used_codes, item.clone());
            if selected.len() >= limit {
                return selected;
            }
        }
    }
    selected
}

fn potential_group_items(
    screened: &[ScreenedStock],
    limit: usize,
    exclude_codes: &HashSet<String>,
    threshold: f64,
) -> Vec<ScreenedStock> {
    let mut ranked = screened.to_vec();
    ranked.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.stock.code.cmp(&right.stock.code))
    });
    let mut selected = Vec::new();
    for item in ranked {
        if exclude_codes.contains(&item.stock.code) || item.score <= threshold {
            continue;
        }
        selected.push(item);
        if selected.len() >= limit {
            break;
        }
    }
    selected
}

fn first_by_theme(
    screened: &[ScreenedStock],
    category: &str,
    used_codes: &HashSet<String>,
) -> Option<ScreenedStock> {
    screened
        .iter()
        .find(|item| {
            !used_codes.contains(&item.stock.code)
                && item.theme_category.as_deref() == Some(category)
        })
        .cloned()
}

fn append_selected(
    selected: &mut Vec<ScreenedStock>,
    used_codes: &mut HashSet<String>,
    item: ScreenedStock,
) {
    used_codes.insert(item.stock.code.clone());
    selected.push(item);
}

fn sort_value(item: &ScreenedStock, sort_by: &str) -> Option<f64> {
    let value = match sort_by {
        "price" => Some(item.stock.price),
        "pe" => item.stock.pe,
        "pb" => item.stock.pb,
        "roe" => item.stock.roe.and_then(ratio_percentage_points),
        "market_cap" | "market_cap_billion" => item.stock.market_cap_billion,
        "change_pct" | "pct" => item.stock.change_pct.and_then(ratio_percentage_points),
        _ => Some(item.score),
    };
    value.filter(|value| value.is_finite())
}
