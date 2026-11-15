use super::*;
use canonical::{bindings, path_text, Graph, References};

fn under(path: &str, parent: &str) -> bool {
    path == parent || path.starts_with(&format!("{parent}::"))
}
fn command(f: &syn::ItemFn) -> bool {
    f.attrs
        .iter()
        .any(|a| path_text(a.path()) == "tauri::command")
}
pub fn forwarding(
    f: &syn::ItemFn,
    resolve: impl Fn(&str) -> Option<String>,
    expected: &str,
) -> bool {
    let [syn::Stmt::Expr(expr, None)] = f.block.stmts.as_slice() else {
        return false;
    };
    let (expr, awaited) = if let syn::Expr::Await(a) = expr {
        (a.base.as_ref(), true)
    } else {
        (expr, false)
    };
    if awaited != f.sig.asyncness.is_some() {
        return false;
    }
    let syn::Expr::Call(call) = expr else {
        return false;
    };
    let syn::Expr::Path(target) = call.func.as_ref() else {
        return false;
    };
    if target.qself.is_some() || resolve(&path_text(&target.path)).as_deref() != Some(expected) {
        return false;
    }
    let parameters = f
        .sig
        .inputs
        .iter()
        .map(|a| {
            let syn::FnArg::Typed(a) = a else { return None };
            let syn::Pat::Ident(p) = a.pat.as_ref() else {
                return None;
            };
            if p.subpat.is_some() {
                return None;
            }
            Some(p.ident.to_string())
        })
        .collect::<Option<Vec<_>>>();
    let Some(parameters) = parameters else {
        return false;
    };
    call.args.len()==parameters.len()&&call.args.iter().zip(parameters).all(|(arg,name)|matches!(arg,syn::Expr::Path(p) if p.qself.is_none()&&p.path.is_ident(&name)))
}

pub fn dependency_forbidden(area: &str, origin: &str, target: &str) -> bool {
    if area == "core" {
        if origin == "crate" {
            return false;
        } // the validated facade explicitly assembles domain exports
        if under(origin, "crate::screening") && under(target, "crate::adaptive_screen") {
            return true;
        }
        let outer = ["crate::agent", "crate::json_api", "crate::ffi"];
        if !outer.iter().any(|owner| under(origin, owner))
            && outer.iter().any(|owner| under(target, owner))
        {
            return true;
        }
        let leaf = [
            "crate::dates",
            "crate::numeric",
            "crate::text",
            "crate::error",
            "crate::market_data",
            "crate::capital_evidence::types",
            "crate::screening::types",
            "crate::trend::types",
            "crate::trend_screen::types",
            "crate::observation::types",
            "crate::backtest::types",
            "crate::agent::types",
            "crate::mobile_skill::types",
            "crate::json_api::requests",
        ];
        let service = [
            "crate::screening::service",
            "crate::screening::scoring",
            "crate::graph_compat::service",
            "crate::backtest::service",
            "crate::backtest::selection",
            "crate::backtest::simulation",
            "crate::trend::service",
            "crate::trend_screen::service",
            "crate::observation::service",
            "crate::adaptive_screen",
            "crate::agent::service",
            "crate::agent::stream",
            "crate::json_api::adapters",
            "crate::mobile_skill::service",
        ];
        return leaf.iter().any(|owner| under(origin, owner))
            && service.iter().any(|owner| under(target, owner));
    }
    if area == "research" {
        if [
            "crate::research::schema",
            "crate::research::validation",
            "crate::research::time",
        ]
        .iter()
        .any(|o| under(origin, o))
        {
            return [
                "crate::research::recovery",
                "crate::research::app_store",
                "crate::research::background",
                "crate::research::commands",
                "crate::research::sources",
            ]
            .iter()
            .any(|o| under(target, o));
        }
        return under(origin, "crate::research::storage")
            && under(target, "crate::research::recovery");
    }
    if area == "market" {
        let leaf = [
            "crate::network",
            "crate::wire_values",
            "crate::market_domain",
            "crate::market_events",
        ];
        if leaf.iter().any(|o| under(origin, o)) {
            return ["crate::market", "crate::cn_market", "crate::hk_market"]
                .iter()
                .any(|o| under(target, o));
        }
        if under(origin, "crate::market::providers") {
            return [
                "crate::market::store",
                "crate::market::cache",
                "crate::market::state",
                "crate::market::refresh",
                "crate::market::commands",
            ]
            .iter()
            .any(|o| under(target, o));
        }
        if under(origin, "crate::market::store") {
            return [
                "crate::market::cache",
                "crate::market::refresh",
                "crate::market::commands",
            ]
            .iter()
            .any(|o| under(target, o));
        }
        if under(origin, "crate::market::cache") {
            return ["crate::market::refresh", "crate::market::commands"]
                .iter()
                .any(|o| under(target, o));
        }
        if under(origin, "crate::market::refresh") {
            return under(target, "crate::market::commands");
        }
    }
    false
}
// Supervisor ruling task-9-packs-role-ruling.json, not a learned directory exemption.
fn reviewed_pack_reference(
    origin: &str,
    reference: &canonical::Reference,
    calls: &[canonical::ResolvedCall],
) -> bool {
    if origin != "crate::research::storage::packs" {
        return false;
    }
    let caller_allowed = |caller: Option<&str>| {
        matches!(
            caller,
            Some(
                "crate::research::storage::packs::import_research_documents"
                    | "crate::research::storage::packs::rollback_research_documents"
            )
        )
    };
    let target_allowed = |target: &str| {
        matches!(
            target,
            "crate::research::recovery::deliver" | "crate::research::recovery::rollback_source"
        )
    };
    if reference.import {
        if reference.caller.is_some() && !caller_allowed(reference.caller.as_deref()) {
            return false;
        }
        return calls.iter().any(|call| {
            caller_allowed(call.caller.as_deref())
                && target_allowed(&call.target)
                && (reference.caller.is_none() || reference.caller == call.caller)
                && (reference.target == "crate::research::recovery"
                    || reference.target == call.target)
        });
    }
    caller_allowed(reference.caller.as_deref()) && target_allowed(&reference.target)
}

pub fn facade_errors(graph: &Graph, module: &str, area: &str, baseline: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    let mut commands = BTreeSet::new();
    let required = baseline[format!("{area}_commands")].as_object();
    for item in &graph.modules[module].items {
        match item {
            Item::Mod(m) => {
                if m.content.is_some() {
                    errors.push(format!(
                        "{area} facade owns production inline module {}",
                        m.ident
                    ))
                }
                if public(&m.vis)
                    || matches!(area, "core" | "research")
                        && !matches!(m.vis, Visibility::Inherited)
                {
                    errors.push(format!(
                        "{area} facade exposes public namespace {}",
                        m.ident
                    ))
                }
                if area == "research"
                    && matches!(m.ident.to_string().as_str(), "recovery" | "hk_cache")
                    && !matches!(m.vis, Visibility::Inherited)
                {
                    errors.push(format!("research::{} must remain private", m.ident))
                }
            }
            Item::Use(u) => {
                if public(&u.vis) && bindings(&u.tree).iter().any(|b| b.glob) {
                    errors.push(format!("{area} facade uses public glob"))
                }
            }
            Item::ExternCrate(_) => {}
            Item::Fn(f) if area != "core" => {
                let name = f.sig.ident.to_string();
                commands.insert(name.clone());
                let expected = format!("{module}::commands::{name}");
                let preserved = required
                    .and_then(|r| r.get(&name))
                    .is_some_and(|s| s.as_str() == Some(signature(item).as_str()));
                if !command(f)
                    || !preserved
                    || !forwarding(f, |p| graph.resolve(module, p), &expected)
                {
                    errors.push(format!(
                        "{area} command {name} has wrong contract/target/forwarding"
                    ))
                }
                match graph.symbols.get(&expected) {
                    Some(Item::Fn(delegate))
                        if !command(delegate)
                            && delegate.sig.asyncness.is_some() == f.sig.asyncness.is_some() => {}
                    _ => errors.push(format!(
                        "{area} command {name} delegate must be same-name plain function"
                    )),
                }
            }
            _ => {
                let (kind, name, _) = info(item);
                errors.push(format!("{area} facade owns {kind} {name}"))
            }
        }
    }
    if let Some(required) = required {
        let required = required.keys().cloned().collect::<BTreeSet<_>>();
        if commands != required {
            errors.push(format!(
                "{area} command identities differ: missing={:?}, extra={:?}",
                required.difference(&commands),
                commands.difference(&required)
            ))
        }
    }
    errors
}

#[derive(Clone, Debug, PartialEq)]
pub struct FingerprintEntry {
    pub label: String,
    pub source: String,
    pub derived: bool,
}
fn fingerprint_entries(call: &syn::ExprCall) -> Result<(Vec<u8>, Vec<FingerprintEntry>), String> {
    if call.args.len() != 2 {
        return Err("fingerprint call must have two arguments".into());
    }
    let syn::Expr::Lit(domain) = &call.args[0] else {
        return Err("fingerprint domain must be literal".into());
    };
    let syn::Lit::ByteStr(domain) = &domain.lit else {
        return Err("fingerprint domain must be byte string".into());
    };
    let syn::Expr::Reference(reference) = &call.args[1] else {
        return Err("fingerprint entries must be referenced array".into());
    };
    let syn::Expr::Array(array) = reference.expr.as_ref() else {
        return Err("fingerprint entries must be array".into());
    };
    let mut entries = Vec::new();
    for entry in &array.elems {
        let syn::Expr::Tuple(pair) = entry else {
            return Err("fingerprint entry must be tuple".into());
        };
        if pair.elems.len() != 2 {
            return Err("fingerprint entry must have two fields".into());
        }
        let syn::Expr::Lit(label) = &pair.elems[0] else {
            return Err("fingerprint label must be literal".into());
        };
        let syn::Lit::Str(label) = &label.lit else {
            return Err("fingerprint label must be string".into());
        };
        let syn::Expr::MethodCall(method) = &pair.elems[1] else {
            return Err("fingerprint bytes must be literal include or derived identity".into());
        };
        if !method.args.is_empty() {
            return Err("fingerprint conversion arguments are forbidden".into());
        }
        if method.method == "as_slice" {
            let syn::Expr::Macro(mac) = method.receiver.as_ref() else {
                return Err("source entry must use include_bytes".into());
            };
            if !mac.mac.path.is_ident("include_bytes") {
                return Err("source entry must use include_bytes".into());
            }
            let path = syn::parse2::<syn::LitStr>(mac.mac.tokens.clone())
                .map_err(|_| "include path must be string literal")?;
            entries.push(FingerprintEntry {
                label: label.value(),
                source: path.value(),
                derived: false,
            });
        } else if method.method == "as_bytes" {
            let syn::Expr::Call(derived) = method.receiver.as_ref() else {
                return Err("derived fingerprint must call identity function".into());
            };
            let syn::Expr::Path(path) = derived.func.as_ref() else {
                return Err("derived identity target must be path".into());
            };
            if !derived.args.is_empty() {
                return Err("derived identity arguments are forbidden".into());
            }
            entries.push(FingerprintEntry {
                label: label.value(),
                source: path_text(&path.path),
                derived: true,
            });
        } else {
            return Err("unknown fingerprint byte conversion".into());
        }
    }
    Ok((domain.value(), entries))
}
pub fn fingerprint_call(
    graph: &Graph,
    module: &str,
    name: &str,
) -> Result<(Vec<u8>, Vec<FingerprintEntry>), String> {
    let owner = format!("{module}::{name}");
    if !matches!(graph.symbols.get(&owner), Some(Item::Fn(_))) {
        return Err("missing fingerprint function".into());
    }
    let calls = References::calls(graph, module)
        .into_iter()
        .filter(|call| {
            call.caller.as_deref() == Some(owner.as_str())
                && matches!(
                    call.target.as_str(),
                    "gp_core::source_text_fingerprint"
                        | "crate::source_fingerprint::source_text_fingerprint"
                )
        })
        .collect::<Vec<_>>();
    if calls.len() != 1 {
        return Err(format!(
            "expected one actual fingerprint call, found {}",
            calls.len()
        ));
    }
    let (domain, mut entries) = fingerprint_entries(&calls[0].expression)?;
    for entry in &mut entries {
        if entry.derived {
            entry.source = calls[0]
                .resolve_name(graph, module, &entry.source)
                .ok_or("unresolved derived identity")?;
        }
    }
    Ok((domain, entries))
}
pub fn validate_fingerprint(graph: &Graph, repo: &Path, descriptor: &Value) -> Vec<String> {
    let module = descriptor["module"].as_str().unwrap();
    let name = descriptor["function"].as_str().unwrap();
    let node = &graph.modules[module];
    let mut errors = Vec::new();
    let (domain, entries) = match fingerprint_call(graph, module, name) {
        Ok(v) => v,
        Err(e) => return vec![format!("{module}::{name}: {e}")],
    };
    let expected_domain = descriptor["domain"].as_str().unwrap().as_bytes();
    if domain != expected_domain {
        errors.push(format!("{name}: fingerprint domain changed"))
    }
    let expected = descriptor["entries"].as_array().unwrap();
    let mut labels = BTreeSet::new();
    let mut sources = BTreeSet::new();
    if entries.len() != expected.len() {
        errors.push(format!("{name}: ordered fingerprint entry count changed"))
    }
    for (index, entry) in entries.iter().enumerate() {
        if !labels.insert(entry.label.clone()) {
            errors.push(format!("{name}: duplicate label {}", entry.label))
        }
        let actual = if entry.derived {
            entry.source.clone()
        } else {
            match node
                .file
                .parent()
                .unwrap()
                .join(&entry.source)
                .canonicalize()
            {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(e) => {
                    errors.push(format!("{name}: missing include {}: {e}", entry.source));
                    continue;
                }
            }
        };
        if !entry.derived && !sources.insert(actual.clone()) {
            errors.push(format!("{name}: duplicate source {}", entry.source))
        }
        if let Some(want) = expected.get(index) {
            let want_source = if want["derived"].as_bool() == Some(true) {
                want["source"].as_str().unwrap().to_string()
            } else {
                repo.join(want["source"].as_str().unwrap())
                    .canonicalize()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            };
            if entry.label != want["label"].as_str().unwrap()
                || entry.derived != want["derived"].as_bool().unwrap_or(false)
                || actual != want_source
            {
                errors.push(format!(
                    "{name}: ordered label/source mismatch at {index}: {} -> {}",
                    entry.label, entry.source
                ))
            }
        }
    }
    if descriptor["all_declared_sources"].as_bool() == Some(true) {
        for node in graph.modules.values() {
            let path = node.file.to_string_lossy().into_owned();
            if !sources.contains(&path) {
                errors.push(format!(
                    "{name}: declared production source absent {}",
                    node.file.display()
                ))
            }
        }
    }
    errors
}

pub fn check(root: &Path, areas: &str) -> Vec<String> {
    let mut errors = Vec::new();
    let selected = |area: &str| areas.split(',').any(|s| s == area || s == "all");
    let baseline: Value = serde_json::from_str(include_str!("public-contracts.json")).unwrap();
    let policy: Value = serde_json::from_str(include_str!("governance-policy.json")).unwrap();
    for (area, lib, module) in [
        ("core", "native/gp-core/src/lib.rs", "crate"),
        (
            "research",
            "desktop/src-tauri/src/lib.rs",
            "crate::research",
        ),
        ("market", "desktop/src-tauri/src/lib.rs", "crate::market"),
    ] {
        if !selected(area) {
            continue;
        }
        if !root.join(lib).exists() {
            errors.push(format!("{area}: missing declared crate root {lib}"));
            continue;
        }
        let graph = Graph::load(&root.join(lib));
        if !graph.modules.contains_key(module) {
            errors.push(format!("{area}: missing facade {module}"));
            continue;
        }
        errors.extend(facade_errors(&graph, module, area, &baseline));
        for origin in graph.modules.keys() {
            let records = References::records(&graph, origin);
            let calls = if area == "research" && origin == "crate::research::storage::packs" {
                References::calls(&graph, origin)
            } else {
                Vec::new()
            };
            for reference in &records {
                if dependency_forbidden(area, origin, &reference.target)
                    && !(area == "research" && reviewed_pack_reference(origin, reference, &calls))
                {
                    let suffix = if !graph.modules.contains_key(&reference.target)
                        && !graph.symbols.contains_key(&reference.target)
                    {
                        graph
                            .known_owner(&reference.target)
                            .map(|owner| format!(" (unresolved suffix under {owner})"))
                            .unwrap_or_default()
                    } else {
                        String::new()
                    };
                    errors.push(format!(
                        "{}:{}: {} depends on {} via {}{}",
                        graph.modules[origin].file.display(),
                        reference.line,
                        reference.caller.as_deref().unwrap_or(origin),
                        reference.target,
                        reference.raw,
                        suffix
                    ))
                }
            }
        }
        if area == "research" {
            let owner = "crate::research::storage::ResearchStore";
            match graph.symbols.get(owner) {
                Some(Item::Struct(s)) if s.fields.len() == 1 && s.generics.params.is_empty() => {
                    let field = s.fields.iter().next().unwrap();
                    let path = if let syn::Type::Path(p) = &field.ty {
                        Some(path_text(&p.path))
                    } else {
                        None
                    };
                    let resolved = path.as_deref().map(|p| {
                        graph.resolve("crate::research::storage", p).as_deref()
                            == Some("std::path::PathBuf")
                    });
                    if field.ident.as_ref().map(ToString::to_string).as_deref() != Some("path")
                        || !matches!(field.vis, Visibility::Inherited)
                        || resolved != Some(true)
                    {
                        errors.push(
                            "ResearchStore must retain its one private path: PathBuf field".into(),
                        )
                    }
                }
                _ => errors
                    .push("ResearchStore must retain its one private path: PathBuf field".into()),
            }
        }
        for descriptor in policy["fingerprints"].as_array().unwrap() {
            if descriptor["area"].as_str() == Some(area) {
                errors.extend(validate_fingerprint(&graph, root, descriptor))
            }
        }
    }
    if selected("core") || areas == "all" {
        let graph = Graph::load(&root.join("native/gp-research/src/lib.rs"));
        for descriptor in policy["fingerprints"].as_array().unwrap() {
            if descriptor["area"].as_str() == Some("native-research") {
                errors.extend(validate_fingerprint(&graph, root, descriptor))
            }
        }
    }
    errors
}
