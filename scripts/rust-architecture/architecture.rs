use proc_macro2::{TokenStream, TokenTree};
use quote::ToTokens;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use syn::{spanned::Spanned, visit::Visit, Item, UseTree, Visibility};
mod canonical;
mod guard_cases;
mod policy;

fn public(vis: &Visibility) -> bool {
    matches!(vis, Visibility::Public(_))
}
fn attrs(a: &[syn::Attribute]) -> String {
    a.iter()
        .filter(|x| !x.path().is_ident("doc"))
        .map(|x| x.to_token_stream().to_string())
        .collect::<Vec<_>>()
        .join(" ")
}
fn signature(item: &Item) -> String {
    match item {
        Item::Fn(x) => {
            let mut s = x.sig.clone();
            for arg in &mut s.inputs {
                if let syn::FnArg::Typed(arg) = arg {
                    if let syn::Pat::Ident(id) = arg.pat.as_mut() {
                        id.mutability = None;
                    }
                }
            }
            format!("{} {}", attrs(&x.attrs), s.to_token_stream())
        }
        Item::Struct(x) => {
            let mut x = x.clone();
            x.attrs.retain(|a| !a.path().is_ident("doc"));
            x.to_token_stream().to_string()
        }
        Item::Enum(x) => {
            let mut x = x.clone();
            x.attrs.retain(|a| !a.path().is_ident("doc"));
            x.to_token_stream().to_string()
        }
        Item::Type(x) => x.to_token_stream().to_string(),
        Item::Const(x) => x.to_token_stream().to_string(),
        Item::Static(x) => x.to_token_stream().to_string(),
        Item::Trait(x) => {
            let mut x = x.clone();
            x.attrs.retain(|a| !a.path().is_ident("doc"));
            for i in &mut x.items {
                if let syn::TraitItem::Fn(m) = i {
                    m.default = None;
                    m.semi_token = Some(Default::default());
                }
            }
            x.to_token_stream().to_string()
        }
        _ => String::new(),
    }
}
fn info(item: &Item) -> (String, String, bool) {
    match item {
        Item::Fn(x) => ("fn".into(), x.sig.ident.to_string(), public(&x.vis)),
        Item::Struct(x) => ("struct".into(), x.ident.to_string(), public(&x.vis)),
        Item::Enum(x) => ("enum".into(), x.ident.to_string(), public(&x.vis)),
        Item::Type(x) => ("type".into(), x.ident.to_string(), public(&x.vis)),
        Item::Trait(x) => ("trait".into(), x.ident.to_string(), public(&x.vis)),
        Item::Const(x) => ("const".into(), x.ident.to_string(), public(&x.vis)),
        Item::Static(x) => ("static".into(), x.ident.to_string(), public(&x.vis)),
        Item::Impl(x) => (
            "impl".into(),
            x.self_ty.to_token_stream().to_string(),
            false,
        ),
        Item::Mod(x) => ("mod".into(), x.ident.to_string(), public(&x.vis)),
        Item::Use(x) => (
            "use".into(),
            x.tree.to_token_stream().to_string(),
            public(&x.vis),
        ),
        Item::Macro(x) => (
            "macro".into(),
            x.ident
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_default(),
            false,
        ),
        _ => ("other".into(), String::new(), false),
    }
}
fn idents(t: TokenStream, out: &mut BTreeSet<String>) {
    for tok in t {
        match tok {
            TokenTree::Ident(i) => {
                out.insert(i.to_string());
            }
            TokenTree::Group(g) => idents(g.stream(), out),
            TokenTree::Literal(l) => {
                if let Ok(s) = syn::parse_str::<syn::LitStr>(&l.to_string()) {
                    out.insert(s.value());
                }
            }
            _ => {}
        }
    }
}
fn use_paths(t: &UseTree, prefix: &str, out: &mut Vec<String>) {
    match t {
        UseTree::Path(p) => use_paths(&p.tree, &format!("{prefix}{}::", p.ident), out),
        UseTree::Name(n) => out.push(format!("{prefix}{}", n.ident)),
        UseTree::Rename(n) => out.push(format!("{prefix}{}", n.ident)),
        UseTree::Glob(_) => out.push(format!("{prefix}*")),
        UseTree::Group(g) => {
            for t in &g.items {
                use_paths(t, prefix, out)
            }
        }
    }
}
#[derive(Default)]
struct Paths(BTreeSet<String>);
impl<'a> Visit<'a> for Paths {
    fn visit_path(&mut self, p: &'a syn::Path) {
        self.0.insert(
            p.segments
                .iter()
                .map(|s| s.ident.to_string())
                .collect::<Vec<_>>()
                .join("::"),
        );
        syn::visit::visit_path(self, p);
    }
    fn visit_macro(&mut self, m: &'a syn::Macro) {
        for expr in canonical::macro_expressions(m.tokens.clone()) {
            self.visit_expr(&expr);
        }
    }
    fn visit_item_use(&mut self, p: &'a syn::ItemUse) {
        let mut v = Vec::new();
        use_paths(&p.tree, "", &mut v);
        self.0.extend(v);
        syn::visit::visit_item_use(self, p);
    }
}
fn parse(p: &Path) -> syn::File {
    syn::parse_file(&fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display())))
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}
fn index(p: &Path) -> Value {
    let f = parse(p);
    Value::Array(f.items.iter().map(|i|{
        let (kind,name,is_public)=info(i);let mut refs=BTreeSet::new();idents(i.to_token_stream(),&mut refs);
        let mut paths=Paths::default();paths.visit_item(i);
        json!({"kind":kind,"name":name,"public":is_public,"start_line":i.span().start().line,"end_line":i.span().end().line,
        "signature":signature(i),"references":refs,"paths":paths.0})
    }).collect())
}
fn exported(file: &Path, _seen: &mut BTreeSet<PathBuf>) -> BTreeMap<String, String> {
    canonical::Graph::load(file).signature_exports()
}
fn command_surface(p: &Path) -> BTreeMap<String, String> {
    parse(p)
        .items
        .iter()
        .filter_map(|i| {
            if let Item::Fn(f) = i {
                if f.attrs.iter().any(|a| {
                    a.path()
                        .segments
                        .iter()
                        .map(|s| s.ident.to_string())
                        .collect::<Vec<_>>()
                        .join("::")
                        == "tauri::command"
                }) {
                    Some((f.sig.ident.to_string(), signature(i)))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}
fn capture(root: &Path) -> Value {
    let core_public = exported(
        &root.join("native/gp-core/src/lib.rs"),
        &mut BTreeSet::new(),
    );
    let methods = public_methods(&root.join("native/gp-core/src/lib.rs"), &core_public);
    json!({"core_public":core_public,"core_methods":methods,
        "research_commands":command_surface(&root.join("desktop/src-tauri/src/research.rs")),
        "market_commands":command_surface(&root.join("desktop/src-tauri/src/market.rs")),
        "core_ffi":command_ffi(&root.join("native/gp-core/src/ffi.rs"))})
}
fn command_ffi(p: &Path) -> BTreeMap<String, String> {
    parse(p)
        .items
        .iter()
        .filter_map(|i| {
            if let Item::Fn(f) = i {
                if f.sig
                    .abi
                    .as_ref()
                    .is_some_and(|a| a.name.as_ref().is_some_and(|n| n.value() == "C"))
                {
                    Some((f.sig.ident.to_string(), signature(i)))
                } else {
                    None
                }
            } else {
                None
            }
        })
        .collect()
}
fn self_test() {
    let file: syn::File = syn::parse_str(
        "#[tauri::command] pub fn request(mut value: i32)->i32{value} pub struct S{pub n:i32}",
    )
    .unwrap();
    let a = signature(&file.items[0]);
    let b = signature(
        &syn::parse_str::<Item>(" #[tauri::command] pub fn request(value:i32)->i32{panic!()}")
            .unwrap(),
    );
    assert_eq!(a, b, "local mutable binding is not public signature");
    assert_ne!(
        signature(&file.items[1]),
        signature(&syn::parse_str::<Item>("pub struct S{pub n:u32}").unwrap())
    );
    let mut p = Paths::default();
    p.visit_file(
        &syn::parse_str(
            "use crate::market::{f,g};fn x(){crate::market::f();let s=\"crate::market::g\";}",
        )
        .unwrap(),
    );
    assert!(p.0.contains("crate::market::f"));
    assert!(p.0.contains("crate::market::g"));
    let mut literal_only = Paths::default();
    literal_only.visit_file(
        &syn::parse_str("fn note(){let s=\"crate::market::f\"; /* crate::market::f() */}").unwrap(),
    );
    assert!(!literal_only.0.contains("crate::market::f"));
    let wrapper = |text: &str| syn::parse_str::<syn::ItemFn>(text).unwrap();
    assert!(thin_wrapper(&wrapper(
        "async fn request(value:i32)->i32{commands::request(value).await}"
    )));
    assert!(!thin_wrapper(&wrapper(
        "fn request(value:i32)->i32{let adjusted=value+1;commands::request(adjusted)}"
    )));
    assert!(!thin_wrapper(&wrapper(
        "fn request(value:i32)->i32{commands::request(||value+1)}"
    )));
    guard_cases::run();
    println!("Rust AST tooling self-tests passed");
}
fn main() {
    let a = std::env::args().skip(1).collect::<Vec<_>>();
    match a.first().map(String::as_str) {
        Some("index") => println!("{}", index(Path::new(&a[1]))),
        Some("capture") => {
            let v = capture(Path::new(&a[1]));
            if let Some(out) = a.get(2) {
                fs::write(out, serde_json::to_string_pretty(&v).unwrap()).unwrap()
            } else {
                println!("{v}")
            }
        }
        Some("check") => {
            let errors = check_architecture(
                Path::new(&a[1]),
                a.get(2).map(String::as_str).unwrap_or("all"),
            );
            for e in errors.iter().take(12) {
                eprintln!("{e}");
            }
            if !errors.is_empty() {
                eprintln!("{} architecture violations", errors.len());
                std::process::exit(1);
            }
            println!("Rust module facade, dependency and fingerprint guards passed");
        }
        Some("compare") => {
            let baseline: Value =
                serde_json::from_str(&fs::read_to_string(&a[2]).unwrap()).unwrap();
            let now = capture(Path::new(&a[1]));
            if baseline != now {
                for k in [
                    "core_public",
                    "core_methods",
                    "research_commands",
                    "market_commands",
                    "core_ffi",
                ] {
                    if baseline[k] != now[k] {
                        eprintln!("Contract differs: {k}");
                        for name in now[k].as_object().unwrap().keys() {
                            if baseline[k].get(name).is_none() {
                                eprintln!("added: {name}");
                            }
                        }
                        for (name, v) in baseline[k].as_object().unwrap() {
                            if now[k][name] != *v {
                                eprintln!(
                                    "changed/missing: {name}\nold: {v}\nnew: {}",
                                    now[k][name]
                                );
                            }
                        }
                    }
                }
                std::process::exit(1);
            }
            println!("Public Rust, Tauri command and C ABI contracts unchanged");
        }
        _ => self_test(),
    }
}

fn public_methods(root: &Path, _exports: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    canonical::Graph::load(root).methods()
}
fn thin_wrapper(f: &syn::ItemFn) -> bool {
    policy::forwarding(
        f,
        |path| Some(path.to_owned()),
        &format!("commands::{}", f.sig.ident),
    )
}
fn check_architecture(root: &Path, areas: &str) -> Vec<String> {
    policy::check(root, areas)
}
