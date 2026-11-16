use super::*;
use canonical::{Graph, References};
type GuardCase = (&'static str, Box<dyn Fn()>);

fn fixture(name: &str, files: &[(&str, &str)]) -> PathBuf {
    let root = PathBuf::from(
        std::env::var("GP_ARCH_FIXTURES").expect("named candidate fixture directory"),
    )
    .join(name);
    for (path, text) in files {
        let dest = root.join(path);
        fs::create_dir_all(dest.parent().unwrap()).unwrap();
        fs::write(dest, text).unwrap();
    }
    root
}

pub fn run() {
    let mut cases: Vec<GuardCase> = vec![
        (
            "wrong wrapper target",
            Box::new(|| {
                let f = syn::parse_str::<syn::ItemFn>(
                    "fn request(value:i32)->i32{commands::other(value)}",
                )
                .unwrap();
                assert!(
                    !thin_wrapper(&f),
                    "a different command must not satisfy forwarding"
                );
            }),
        ),
        (
            "swapped wrapper parameters",
            Box::new(|| {
                let f = syn::parse_str::<syn::ItemFn>(
                    "fn request(a:i32,b:i32)->i32{commands::request(b,a)}",
                )
                .unwrap();
                assert!(
                    !thin_wrapper(&f),
                    "same-typed arguments must keep identity/order"
                );
            }),
        ),
        (
            "public alias name",
            Box::new(|| {
                let root = fixture(
                    "alias",
                    &[
                        ("lib.rs", "mod owner; pub use owner::StockItem as Item;"),
                        ("owner.rs", "pub struct StockItem { pub code: String }"),
                    ],
                );
                let exports = exported(&root.join("lib.rs"), &mut BTreeSet::new());
                assert!(exports.contains_key("Item"));
                assert!(!exports.contains_key("StockItem"));
            }),
        ),
        (
            "qualified public exports",
            Box::new(|| {
                let root = fixture(
                    "qualified",
                    &[
                        ("lib.rs", "mod owner; pub use crate::owner::StockItem;"),
                        ("owner.rs", "pub struct StockItem;"),
                    ],
                );
                assert!(
                    exported(&root.join("lib.rs"), &mut BTreeSet::new()).contains_key("StockItem")
                );
            }),
        ),
        (
            "private same-named type",
            Box::new(|| {
                let root = fixture(
                    "methods",
                    &[
                        ("lib.rs", "mod owner; mod zprivate; pub use owner::S;"),
                        ("owner.rs", "pub struct S; impl S { pub fn go()->i32{0} }"),
                        ("zprivate.rs", "struct S; impl S { pub fn go()->u32{0} }"),
                    ],
                );
                let exports = exported(&root.join("lib.rs"), &mut BTreeSet::new());
                assert!(public_methods(&root.join("lib.rs"), &exports)["S::go"].contains("i32"));
            }),
        ),
        (
            "ordinary macro expression path",
            Box::new(|| {
                let mut paths = Paths::default();
                paths.visit_file(
                    &syn::parse_str("fn x(){format!(\"{}\",crate::market::epoch_millis());}")
                        .unwrap(),
                );
                assert!(paths.0.contains("crate::market::epoch_millis"));
            }),
        ),
        (
            "production inline facade hiding",
            Box::new(|| {
                for (name, source, inline) in [
                    ("inline-root", "mod hidden {fn business(){}}", true),
                    ("out-of-line-root", "mod hidden;", false),
                ] {
                    let root = fixture(
                        name,
                        &[
                            ("desktop/src-tauri/src/lib.rs", "mod research;"),
                            ("desktop/src-tauri/src/research.rs", source),
                            (
                                "desktop/src-tauri/src/research/hidden.rs",
                                "fn business(){}",
                            ),
                        ],
                    );
                    let graph = Graph::load(&root.join("desktop/src-tauri/src/lib.rs"));
                    let errors = policy::facade_errors(
                        &graph,
                        "crate::research",
                        "research",
                        &json!({"research_commands":{}}),
                    );
                    assert_eq!(
                        errors
                            .iter()
                            .any(|e| e == "research facade owns production inline module hidden"),
                        inline,
                        "{name}: {errors:?}"
                    );
                    if !inline {
                        assert!(errors.is_empty(), "{errors:?}");
                    }
                }
            }),
        ),
        (
            "block alias chains",
            Box::new(|| {
                let root = dependency_fixture(
                    "block-alias",
                    "fn x(){use crate::market as m;use m::store::read as r;format!(\"{}\",r());}",
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(References::collect(&graph, "crate::market::providers")
                    ["crate::market::store::read"]
                    .contains("r"));
            }),
        ),
        (
            "block glob imported function",
            Box::new(|| {
                let root = dependency_fixture(
                    "block-glob",
                    "fn x(){use crate::market::store::*;format!(\"{}\",read());}",
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(References::collect(&graph, "crate::market::providers")
                    ["crate::market::store::read"]
                    .contains("read"));
            }),
        ),
        (
            "closure shadows import alias",
            Box::new(|| {
                let root = dependency_fixture(
                    "closure-shadow",
                    "use crate::market as m;fn x(){let closure=|m:i32|m;closure(0);}",
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(
                    !References::collect(&graph, "crate::market::providers")["crate::market"]
                        .contains("m")
                );
            }),
        ),
        (
            "value binding cannot hide qualified module path",
            Box::new(|| {
                let root = dependency_fixture(
                    "value-qualified",
                    "use crate::market as m;fn x(){let m=0;format!(\"{}\",m::store::read());}",
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(References::collect(&graph, "crate::market::providers")
                    .contains_key("crate::market::store::read"));
            }),
        ),
        (
            "local function shadows imported value function",
            Box::new(|| {
                let root = dependency_fixture(
                    "local-function-shadow",
                    "use crate::market::store::read;fn x(){fn read()->i32{0}read();}",
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(!References::collect(&graph, "crate::market::providers")
                    ["crate::market::store::read"]
                    .contains("read"));
            }),
        ),
        (
            "provider logical path aliases and macro paths",
            Box::new(|| {
                for (name,text) in [("module-alias","use crate::market as m;fn x(){format!(\"{}\",m::store::read());}"),("grouped-rename","use super::store::{self as s,read as r};fn x(){json!({\"value\":s::read(),\"other\":r()});}"),("relative","fn x(){super::store::read();}")] {
                let root=dependency_fixture(name,text);let graph=Graph::load(&root.join("lib.rs"));
                let paths=References::collect(&graph,"crate::market::providers");
                assert!(paths.contains_key("crate::market::store::read"),"{name}");
                assert!(policy::dependency_forbidden("market","crate::market::providers","crate::market::store::read"));
            }
            }),
        ),
        (
            "repeated super in path-declared provider descendant",
            Box::new(|| {
                let root = dependency_fixture("super-descendant", "mod nested;");
                fixture(
                    "super-descendant",
                    &[(
                        "provider-alias/nested.rs",
                        "use super::super::store::read as r;fn x(){format!(\"{}\",r());}",
                    )],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert_eq!(
                    graph
                        .resolve(
                            "crate::market::providers::nested",
                            "super::super::store::read"
                        )
                        .as_deref(),
                    Some("crate::market::store::read")
                );
                assert!(
                    References::collect(&graph, "crate::market::providers::nested")
                        .contains_key("crate::market::store::read")
                );
            }),
        ),
        (
            "allowed dependency aliases",
            Box::new(|| {
                let root=dependency_fixture("allowed-owner","use crate::network::{self as n,allowed as a};fn x(){format!(\"{}\",n::allowed());a();}");
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(References::collect(&graph, "crate::market::providers")
                    .keys()
                    .all(|p| !policy::dependency_forbidden(
                        "market",
                        "crate::market::providers",
                        p
                    )));
            }),
        ),
        (
            "core/research direction boundaries",
            Box::new(|| {
                assert!(policy::dependency_forbidden(
                    "core",
                    "crate::screening::service",
                    "crate::adaptive_screen::adaptive_screen_stocks"
                ));
                assert!(!policy::dependency_forbidden(
                    "core",
                    "crate::backtest::selection",
                    "crate::adaptive_screen::adaptive_screen_stocks"
                ));
                assert!(policy::dependency_forbidden(
                    "core",
                    "crate::market_data",
                    "crate::agent::service::run_agent_with_data"
                ));
                assert!(!policy::dependency_forbidden(
                    "core",
                    "crate::observation::types",
                    "crate::screening::types::default_tone"
                ));
                assert!(policy::dependency_forbidden(
                    "research",
                    "crate::research::schema",
                    "crate::research::recovery::deliver"
                ));
                assert!(policy::dependency_forbidden(
                    "research",
                    "crate::research::storage",
                    "crate::research::recovery::deliver"
                ));
                assert!(!policy::dependency_forbidden(
                    "research",
                    "crate::research::recovery",
                    "crate::research::storage::ResearchStore"
                ));
            }),
        ),
        (
            "narrow reviewed free pack delivery roles",
            Box::new(|| {
                for (name,body,allowed) in [
                ("free-delivery","fn import_research_documents(){recovery::deliver();}fn rollback_research_documents(){recovery::rollback_source();}",true),
                ("wrong-free-function","fn connection(){recovery::deliver();}",false),
                ("inherent-spoof","impl super::ResearchStore {fn import_research_documents(){recovery::deliver();}}",false),
                ("nested-spoof","fn other(){fn import_research_documents(){recovery::deliver();}}",false),
                ("initializer","static VALUE:()={recovery::deliver()};",false),
            ] {
                let name=format!("packs-{name}");let pack=format!("use crate::research::recovery;{body}");
                let root=fixture(&name,&[("desktop/src-tauri/src/lib.rs","mod research;"),("desktop/src-tauri/src/research.rs","mod storage;mod recovery;"),("desktop/src-tauri/src/research/storage.rs","mod packs;pub struct ResearchStore{path:std::path::PathBuf}"),("desktop/src-tauri/src/research/recovery.rs","pub fn deliver(){} pub fn rollback_source(){}"),("desktop/src-tauri/src/research/storage/packs.rs",&pack)]);
                let errors=policy::check(&root,"research");
                assert_eq!(!errors.iter().any(|e|e.contains("depends on")),allowed,"{name}: {errors:?}");
            }
            }),
        ),
        (
            "cfg test classification retains production feature branch",
            Box::new(|| {
                let root=fixture("cfg-union",&[("lib.rs","#[cfg(test)]mod tests;#[cfg(all(test,windows))]mod other_tests;#[cfg(any(test,feature=\"live\"))]mod feature;#[cfg(windows)]mod win;"),("feature.rs","pub fn f(){}"),("win.rs","pub fn g(){}")]);
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(!graph.modules.contains_key("crate::tests"));
                assert!(!graph.modules.contains_key("crate::other_tests"));
                assert!(graph.modules.contains_key("crate::feature"));
                assert!(graph.modules.contains_key("crate::win"));
            }),
        ),
        (
            "conditional owned imports remain visible",
            Box::new(|| {
                let root=dependency_fixture("conditional-import","#[cfg(windows)]use crate::market::store::read as r;#[cfg(windows)]fn x(){format!(\"{}\",r());}#[cfg(not(windows))]fn x(){ }");
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(References::collect(&graph, "crate::market::providers")
                    .contains_key("crate::market::store::read"));
            }),
        ),
        (
            "nested inline path directory",
            Box::new(|| {
                let root = fixture(
                    "inline-path",
                    &[
                        ("lib.rs", "mod holder;"),
                        ("holder.rs", "mod inner {#[path=\"child.rs\"]mod nested;}"),
                        ("holder/inner/child.rs", "pub struct S;"),
                    ],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(graph
                    .symbols
                    .contains_key("crate::holder::inner::nested::S"));
            }),
        ),
        (
            "cycle and unresolved module fail closed",
            Box::new(|| {
                let root = fixture("cycle", &[("lib.rs", "#[path=\"lib.rs\"]mod cycle;")]);
                assert!(rejected(|| Graph::load(&root.join("lib.rs"))));
                let root = fixture("missing-module", &[("lib.rs", "mod absent;")]);
                assert!(rejected(|| Graph::load(&root.join("lib.rs"))));
            }),
        ),
        (
            "cyclic glob traversal terminates",
            Box::new(|| {
                let root = fixture(
                    "glob-cycle",
                    &[
                        ("lib.rs", "mod owner;pub use owner::*;"),
                        ("owner.rs", "use crate::*;pub struct S;fn x(){unknown();}"),
                    ],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert_eq!(
                    graph.resolve("crate::owner", "S").as_deref(),
                    Some("crate::owner::S")
                );
                assert!(graph.resolve("crate::owner", "unknown").is_none());
            }),
        ),
        (
            "renamed type owner retains methods",
            Box::new(|| {
                let root = fixture(
                    "method-alias",
                    &[
                        (
                            "lib.rs",
                            "mod owner;mod implementation;pub use self::owner::S as Item;",
                        ),
                        ("owner.rs", "pub struct S;"),
                        (
                            "implementation.rs",
                            "use crate::owner::S as Actual;impl Actual {pub fn go()->i32{0}}",
                        ),
                    ],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(graph.methods().contains_key("Item::go"));
                assert!(!graph.methods().contains_key("S::go"));
            }),
        ),
        (
            "thin wrapper negative and alias controls",
            Box::new(|| {
                for body in [
                    "commands::request(b,a)",
                    "commands::other(a,b)",
                    "commands::request(a)",
                    "commands::request(a,b,a)",
                    "commands::request(a,crate::VALUE)",
                    "commands::request(a+1,b)",
                    "commands::request(||a,b)",
                    "let x=a;commands::request(x,b)",
                    "commands::request(a,b).await",
                ] {
                    let f = syn::parse_str::<syn::ItemFn>(&format!(
                        "fn request(a:i32,b:i32)->i32{{{body}}}"
                    ))
                    .unwrap();
                    assert!(!thin_wrapper(&f), "{body}");
                }
                let root=fixture("wrapper-alias",&[("lib.rs","mod market;"),("market.rs","mod commands;use self::commands::request as send;#[tauri::command]async fn request(a:i32,b:i32)->i32{send(a,b).await}"),("market/commands.rs","pub async fn request(a:i32,b:i32)->i32{a+b}")]);
                let graph = Graph::load(&root.join("lib.rs"));
                let item = graph.symbols["crate::market::request"].clone();
                let baseline = json!({"market_commands":{"request":signature(&item)}});
                assert!(
                    policy::facade_errors(&graph, "crate::market", "market", &baseline).is_empty()
                );
                let Item::Fn(mut f) = item else { panic!() };
                f.attrs.clear();
                graph_fails_wrapper(&root, &f, &baseline);
            }),
        ),
        (
            "public glob and namespace facade rejection",
            Box::new(|| {
                let root = fixture(
                    "public-facade",
                    &[
                        ("lib.rs", "pub mod owner;pub use owner::*;"),
                        ("owner.rs", "pub struct S;"),
                    ],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                let errors = policy::facade_errors(&graph, "crate", "core", &json!({}));
                assert!(errors.iter().any(|e| e.contains("namespace")));
                assert!(errors.iter().any(|e| e.contains("glob")));
            }),
        ),
        (
            "core root namespace keeps private visibility",
            Box::new(|| {
                let root = fixture(
                    "restricted-root",
                    &[
                        ("lib.rs", "pub(crate)mod owner;"),
                        ("owner.rs", "pub struct S;"),
                    ],
                );
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(policy::facade_errors(&graph, "crate", "core", &json!({}))
                    .iter()
                    .any(|e| e.contains("namespace")));
            }),
        ),
        (
            "Store PathBuf shape and alias controls",
            Box::new(|| {
                for (name, storage, valid) in [
                    (
                        "field-alias",
                        "use std::path::PathBuf as Buf;pub struct ResearchStore{path:Buf}",
                        true,
                    ),
                    (
                        "module-alias",
                        "use std::path as p;pub struct ResearchStore{path:p::PathBuf}",
                        true,
                    ),
                    (
                        "connection-field",
                        "pub struct ResearchStore{path:rusqlite::Connection}",
                        false,
                    ),
                    (
                        "widened-field",
                        "pub struct ResearchStore{pub(crate)path:std::path::PathBuf}",
                        false,
                    ),
                    (
                        "generic-shadow",
                        "use std::path::PathBuf;pub struct ResearchStore<PathBuf>{path:PathBuf}",
                        false,
                    ),
                ] {
                    let name = format!("store-shape-{name}");
                    let root = fixture(
                        &name,
                        &[
                            ("desktop/src-tauri/src/lib.rs", "mod research;"),
                            ("desktop/src-tauri/src/research.rs", "mod storage;"),
                            ("desktop/src-tauri/src/research/storage.rs", storage),
                        ],
                    );
                    let errors = policy::check(&root, "research");
                    assert_eq!(
                        !errors
                            .iter()
                            .any(|e| e.contains("ResearchStore must retain")),
                        valid,
                        "{name}"
                    );
                }
            }),
        ),
        (
            "ordered actual fingerprint boundary cases",
            Box::new(|| {
                for (name,call,valid) in [
                ("valid","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice()),(\"b\",include_bytes!(\"b.rs\").as_slice())]);",true),
                ("decoy","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);",false),
                ("duplicate-label","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice()),(\"a\",include_bytes!(\"b.rs\").as_slice())]);",false),
                ("duplicate-source","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice()),(\"b\",include_bytes!(\"./a.rs\").as_slice())]);",false),
                ("reorder","fp(b\"test\\0\",&[(\"b\",include_bytes!(\"b.rs\").as_slice()),(\"a\",include_bytes!(\"a.rs\").as_slice())]);",false),
                ("wrong-pair","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"b.rs\").as_slice()),(\"b\",include_bytes!(\"a.rs\").as_slice())]);",false),
                ("domain","fp(b\"wrong\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice()),(\"b\",include_bytes!(\"b.rs\").as_slice())]);",false),
                ("two-calls","fp(b\"test\\0\",&[]);fp(b\"test\\0\",&[]);",false),
                ("computed-array","fp(b\"test\\0\",&ENTRIES);",false),
            ] {
                let name=format!("fingerprint-{name}");let source=format!("use gp_core::source_text_fingerprint as fp;fn identity(){{{call}}}fn decoy(){{include_bytes!(\"b.rs\");}}");
                let root=fixture(&name,&[("lib.rs","mod finger;"),("finger.rs",&source),("a.rs","a"),("b.rs","b")]);let graph=Graph::load(&root.join("lib.rs"));
                let descriptor=json!({"module":"crate::finger","function":"identity","domain":"test\0","all_declared_sources":false,"entries":[{"label":"a","source":"a.rs"},{"label":"b","source":"b.rs"}]});
                assert_eq!(policy::validate_fingerprint(&graph,&root,&descriptor).is_empty(),valid,"{name}");
            }
            }),
        ),
    ];
    cases.extend(round1_cases());
    cases.extend(round2_cases());
    cases.extend(round3_cases());
    let mut failed = Vec::new();
    for (name, case) in cases {
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(case)).is_err() {
            failed.push(name);
        } else {
            println!("guard case passed: {name}");
        }
    }
    assert!(failed.is_empty(), "guard cases failed: {failed:?}");
}

fn round1_cases() -> Vec<GuardCase> {
    vec![
        (
            "round1 qself and governed unresolved owners",
            Box::new(|| {
                for (name, source, blocked) in [
                    ("direct", "fn x(){crate::market::store::S::go();}", true),
                    ("qself", "fn x(){<crate::market::store::S>::go();}", true),
                    (
                        "qself-trait",
                        "fn x(){<crate::market::store::S as crate::network::Trait>::go();}",
                        true,
                    ),
                    (
                        "generated",
                        "fn x(){crate::market::store::generated();}",
                        true,
                    ),
                    (
                        "generated-module-alias",
                        "use crate::market as m;fn x(){m::store::generated();}",
                        true,
                    ),
                    (
                        "generated-function-alias",
                        "use crate::market::store::generated as f;fn x(){f();}",
                        true,
                    ),
                    ("allowed-qself", "fn x(){<crate::network::S>::go();}", false),
                    (
                        "global-value-does-not-hide-module",
                        "use crate::market as m;fn m(){}fn x(){m::store::generated();}",
                        true,
                    ),
                    (
                        "allowed-trait-qself",
                        "fn x(){<crate::network::S as crate::network::Trait>::go();}",
                        false,
                    ),
                    ("external", "fn x(){serde_json::generated();}", false),
                ] {
                    let root = round1_reference_fixture(name, source);
                    let graph = Graph::load(&root.join("lib.rs"));
                    let records = References::records(&graph, "crate::market::providers");
                    assert_eq!(
                        records
                            .iter()
                            .filter(|r| !r.import)
                            .any(|r| policy::dependency_forbidden(
                                "market",
                                "crate::market::providers",
                                &r.target
                            )),
                        blocked,
                        "{name}"
                    );
                }
            }),
        ),
        (
            "round1 type and pattern lexical namespaces",
            Box::new(|| {
                for (name,source,raw) in [
                ("generic","use crate::network::S as Alias;fn x<Alias:crate::network::Trait>(){Alias::go();}","Alias::go"),
                ("localtype","use crate::network::S as Alias;fn x(){struct Alias;impl Alias{fn go(){}}Alias::go();}","Alias::go"),
                ("for","use crate::network::read as item;fn x(v:Vec<fn()>){for item in v{item();}}","item"),
                ("match","use crate::network::read as item;fn x(v:fn()){match v{item=>item()}}","item"),
                ("match-guard","use crate::network::read as item;fn x(v:fn()){match v{item if {item();true}=>item(),_=>()}}","item"),
                ("if-let","use crate::network::read as item;fn x(v:fn()){if let item=v{item();}}","item"),
                ("while-let","use crate::network::read as item;fn x(v:Option<fn()>){while let Some(item)=v{item();break}}","item"),
            ] {
                let root=round1_reference_fixture(name,source);let graph=Graph::load(&root.join("lib.rs"));
                assert!(!References::records(&graph,"crate::market::providers").iter().any(|r|!r.import&&r.raw==raw),"{name}");
            }
                let root=round1_reference_fixture("type-restoration","use crate::network::S as Alias;fn x(){{struct Alias;impl Alias{fn go(){}}Alias::go();}Alias::go();}");
                let graph = Graph::load(&root.join("lib.rs"));
                assert_eq!(
                    References::records(&graph, "crate::market::providers")
                        .iter()
                        .filter(|r| !r.import && r.raw == "Alias::go")
                        .count(),
                    1
                );
            }),
        ),
        (
            "round1 fingerprint callable and lexical identity",
            Box::new(|| {
                for (name,body,valid) in [
                ("original","fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);",true),
                ("nested","fn decoy(){fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);}",false),
                ("shadow","fn fp(_: &[u8],_: &[(&str,&[u8])]){}fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);",false),
                ("blockalias","use gp_core::source_text_fingerprint as local;local(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);",true),
                ("closure","let cell=std::sync::OnceLock::new();let _=cell.get_or_init(||{fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);});",true),
            ] {
                let name=format!("round1-fp-{name}");let source=format!("use gp_core::source_text_fingerprint as fp;fn identity(){{{body}}}");
                let root=fixture(&name,&[("lib.rs","mod finger;"),("finger.rs",&source),("a.rs","a")]);let graph=Graph::load(&root.join("lib.rs"));
                let descriptor=json!({"module":"crate::finger","function":"identity","domain":"test\0","entries":[{"label":"a","source":"a.rs"}],"all_declared_sources":false});
                assert_eq!(policy::validate_fingerprint(&graph,&root,&descriptor).is_empty(),valid,"{name}");
            }
            }),
        ),
        (
            "round1 exact imported pack function allowance",
            Box::new(|| {
                for (name,source,valid) in [
                ("allowed","use crate::research::recovery::deliver as d;fn import_research_documents(){d();}",true),
                ("wrongcaller","use crate::research::recovery::deliver as d;fn other(){d();}",false),
                ("unused","use crate::research::recovery::deliver as d;fn import_research_documents(){}",false),
                ("wrongtarget","use crate::research::recovery::deliver as d;use crate::research::recovery::rollback_source as r;fn import_research_documents(){r();}",false),
            ] {
                let name=format!("round1-pack-{name}");let root=fixture(&name,&[("desktop/src-tauri/src/lib.rs","mod research;"),("desktop/src-tauri/src/research.rs","mod storage;mod recovery;"),("desktop/src-tauri/src/research/storage.rs","mod packs;pub struct ResearchStore{path:std::path::PathBuf}"),("desktop/src-tauri/src/research/recovery.rs","pub fn deliver(){}pub fn rollback_source(){}"),("desktop/src-tauri/src/research/storage/packs.rs",source)]);
                assert_eq!(!policy::check(&root,"research").iter().any(|e|e.contains("depends on")),valid,"{name}");
            }
            }),
        ),
    ]
}
fn round1_reference_fixture(name: &str, provider: &str) -> PathBuf {
    let name = format!("round1-ref-{name}");
    fixture(&name,&[("lib.rs","mod market;mod network;"),("market.rs","mod store;mod providers;"),("market/store.rs","pub struct S;impl S{pub fn go(){}}impl crate::network::Trait for S{fn go(){}}macro_rules! make{()=>{pub fn generated(){}}}make!();"),("network.rs","pub struct S;impl S{pub fn go(){}}pub trait Trait{fn go();}impl Trait for S{fn go(){}}pub fn read(){}"),("market/providers.rs",provider)])
}

fn round2_cases() -> Vec<GuardCase> {
    vec![
        (
            "round2 explicit block owner retains unknown suffix",
            Box::new(|| {
                for (name, target, bad) in [
                    ("block", "r::generated()", true),
                    ("direct", "crate::research::recovery::generated()", true),
                    ("allowed", "r::deliver()", false),
                ] {
                    let source=format!("use crate::research::recovery;fn import_research_documents(){{use crate::research::recovery as r;r::deliver();{target};}}");
                    let root=fixture(&format!("round2-owner-{name}"),&[("desktop/src-tauri/src/lib.rs","mod research;"),("desktop/src-tauri/src/research.rs","mod storage;mod recovery;"),("desktop/src-tauri/src/research/storage.rs","mod packs;pub struct ResearchStore{path:std::path::PathBuf}"),("desktop/src-tauri/src/research/recovery.rs","pub fn deliver(){}pub fn rollback_source(){}macro_rules! m{()=>{pub fn generated(){}}}m!();"),("desktop/src-tauri/src/research/storage/packs.rs",&source)]);
                    let errors = policy::check(&root, "research");
                    assert_eq!(
                        errors.iter().any(|e| e.contains("depends on")),
                        bad,
                        "{name}: {errors:?}"
                    );
                }
            }),
        ),
        (
            "round2 innermost imports and shadows share lexical precedence",
            Box::new(|| {
                for (name,body,valid) in [
            ("value-import","fn fp(_: &[u8],_: &[(&str,&[u8])]){} {use gp_core::source_text_fingerprint as fp;fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);}",true),
            ("value-shadow","{use gp_core::source_text_fingerprint as fp;{fn fp(_: &[u8],_: &[(&str,&[u8])]){}fp(b\"test\\0\",&[(\"a\",include_bytes!(\"a.rs\").as_slice())]);}}",false),
        ]{
            let source=format!("fn identity(){{{body}}}");let root=fixture(&format!("round2-fp-{name}"),&[("lib.rs","mod finger;"),("finger.rs",&source),("a.rs","a")]);let graph=Graph::load(&root.join("lib.rs"));
            let descriptor=json!({"module":"crate::finger","function":"identity","domain":"test\0","entries":[{"label":"a","source":"a.rs"}],"all_declared_sources":false});
            assert_eq!(policy::validate_fingerprint(&graph,&root,&descriptor).is_empty(),valid,"{name}");
        }
                for (name,source,count) in [
            ("type-import","fn x(){struct Alias;{use crate::network::S as Alias;Alias::go();}}",1),
            ("type-shadow","fn x(){use crate::network::S as Alias;{struct Alias;impl Alias{fn go(){}}Alias::go();}}",0),
            ("type-restoration","use crate::network::S as Alias;fn x(){{struct Alias;{use crate::network::S as Alias;Alias::go();}}Alias::go();}",2),
        ]{
            let root=round1_reference_fixture(&format!("round2-{name}"),source);let graph=Graph::load(&root.join("lib.rs"));
            assert_eq!(References::records(&graph,"crate::market::providers").iter().filter(|r|!r.import&&r.raw=="Alias::go").count(),count,"{name}");
        }
            }),
        ),
        (
            "round2 generic declaration scopes and restoration",
            Box::new(|| {
                for (name,source,raw) in [
            ("struct","use crate::network::S as Alias;struct D<Alias>{field:Alias}","Alias"),
            ("enum","use crate::network::S as Alias;enum D<Alias>{V(Alias)}","Alias"),
            ("alias","use crate::network::S as Alias;type D<Alias>=Option<Alias>;","Alias"),
            ("trait","use crate::network::S as Alias;trait D<Alias>{fn f(x:Alias);}","Alias"),
            ("const-struct","use crate::network::read as N;struct D<const N:usize>{field:[u8;N]}","N"),
            ("const-enum","use crate::network::read as N;enum D<const N:usize>{V([u8;N])}","N"),
            ("const-alias","use crate::network::read as N;type D<const N:usize>=[u8;N];","N"),
            ("const-trait","use crate::network::read as N;trait D<const N:usize>{const VALUE:usize=N;}","N"),
        ]{
            let root=round1_reference_fixture(&format!("round2-{name}"),source);let graph=Graph::load(&root.join("lib.rs"));
            assert!(!References::records(&graph,"crate::market::providers").iter().any(|r|!r.import&&r.raw==raw),"{name}");
        }
                for (name, declaration) in [
                    ("struct", "struct D<Alias>{field:Alias}"),
                    ("enum", "enum D<Alias>{V(Alias)}"),
                    ("alias", "type D<Alias>=Option<Alias>;"),
                    ("trait", "trait D<Alias>{fn f(x:Alias);}"),
                    ("trait-method", "trait D{fn f<Alias>(x:Alias);}"),
                    ("trait-associated", "trait D{type Item<Alias>:Into<Alias>;}"),
                    ("impl-associated", "trait D{type Item<T>;}struct Owner;impl D for Owner{type Item<Alias>=Alias;}"),
                ] {
                    let source=format!("use crate::network::S as Alias;\n{declaration}\nstruct Actual{{field:Alias}}");
                    let root = round1_reference_fixture(&format!("round2-restore-{name}"), &source);
                    let graph = Graph::load(&root.join("lib.rs"));
                    let records = References::records(&graph, "crate::market::providers");
                    assert!(
                        !records
                            .iter()
                            .any(|r| !r.import && r.raw == "Alias" && r.line == 2),
                        "{name} generic"
                    );
                    assert!(
                        records.iter().any(|r| !r.import
                            && r.raw == "Alias"
                            && r.line == 3
                            && r.target == "crate::network::S"),
                        "{name} restoration"
                    );
                }
                for (name, declaration) in [
                    ("enum", "enum D{V(Alias)}"),
                    ("alias", "type D=Alias;"),
                    ("trait", "trait D{fn f(x:Alias);}"),
                ] {
                    let source = format!("use crate::network::S as Alias;{declaration}");
                    let root =
                        round1_reference_fixture(&format!("round2-positive-{name}"), &source);
                    let graph = Graph::load(&root.join("lib.rs"));
                    assert!(
                        References::records(&graph, "crate::market::providers")
                            .iter()
                            .any(|r| !r.import
                                && r.raw == "Alias"
                                && r.target == "crate::network::S"),
                        "{name}"
                    );
                }
            }),
        ),
    ]
}

fn round3_cases() -> Vec<GuardCase> {
    vec![
        (
            "round3 module alias chain retains approved and unknown owners",
            Box::new(|| {
                for (name, body, bad) in [
                    ("approved", "alias::deliver();", false),
                    ("unknown", "r::deliver();alias::generated();", true),
                    (
                        "qualified",
                        "r::deliver();crate::research::recovery::generated();",
                        true,
                    ),
                ] {
                    let pack=format!("fn import_research_documents(){{use crate::research::recovery as r;use r as alias;{body}}}");
                    let root=fixture(&format!("round3-pack-{name}"),&[("desktop/src-tauri/src/lib.rs","mod research;"),("desktop/src-tauri/src/research.rs","mod recovery;mod storage;"),("desktop/src-tauri/src/research/recovery.rs","pub fn deliver(){}pub fn rollback_source(){}macro_rules! m{()=>{pub fn generated(){}}}m!();"),("desktop/src-tauri/src/research/storage.rs","mod packs;pub struct ResearchStore{path:std::path::PathBuf}"),("desktop/src-tauri/src/research/storage/packs.rs",&pack)]);
                    let graph = Graph::load(&root.join("desktop/src-tauri/src/lib.rs"));
                    let records = References::records(&graph, "crate::research::storage::packs");
                    assert!(
                        records.iter().any(|r| r.import
                            && r.raw == "r"
                            && r.target == "crate::research::recovery"),
                        "{name} import"
                    );
                    assert_eq!(
                        policy::check(&root, "research")
                            .iter()
                            .any(|e| e.contains("depends on")),
                        bad,
                        "{name}"
                    );
                }
            }),
        ),
        (
            "round3 module import chain namespace and lexical controls",
            Box::new(|| {
                for (name,source,raw,expected) in [
            ("inner-type","fn x(){use crate::network as r;use r as alias;{struct alias;impl alias{fn read(){}}alias::read();}}","alias::read",false),
            ("inner-import","fn x(){use crate::network as r;struct alias;{use r as alias;alias::read();}}","alias::read",true),
            ("local-value","fn x(){use crate::network as r;use r as alias;let alias=0;alias::read();}","alias::read",true),
            ("function-chain","fn x(){use crate::network::read as r;use r as alias;alias();}","alias",true),
            ("external","fn x(){use std as r;use r as alias;alias::mem::drop(());}","alias::mem::drop",true),
        ]{
            let root=round1_reference_fixture(&format!("round3-{name}"),source);let graph=Graph::load(&root.join("lib.rs"));
            let records=References::records(&graph,"crate::market::providers");
            assert_eq!(records.iter().any(|r|!r.import&&r.raw==raw),expected,"{name}");
            if name=="function-chain" {assert!(records.iter().any(|r|r.import&&r.raw=="r"&&r.target=="crate::network::read"));}
            if name=="external" {assert!(!records.iter().filter(|r|!r.import).any(|r|policy::dependency_forbidden("market","crate::market::providers",&r.target)));}
        }
                let root=round1_reference_fixture("round3-ambiguity","fn x(){use crate::network::read as r;use crate::market::store::other as r;use r as alias;alias();}");
                let store = root.join("market/store.rs");
                let mut source = fs::read_to_string(&store).unwrap();
                source.push_str("pub fn other(){}");
                fs::write(store, source).unwrap();
                let graph = Graph::load(&root.join("lib.rs"));
                assert!(rejected(|| References::records(
                    &graph,
                    "crate::market::providers"
                )));
            }),
        ),
    ]
}

fn dependency_fixture(name: &str, provider: &str) -> PathBuf {
    fixture(
        name,
        &[
            ("lib.rs", "mod market;mod network;"),
            (
                "market.rs",
                "mod store;#[path=\"provider-alias.rs\"]mod providers;",
            ),
            ("market/store.rs", "pub fn read()->i32{0}"),
            ("provider-alias.rs", provider),
            ("network.rs", "pub fn allowed()->i32{0}"),
        ],
    )
}

fn graph_fails_wrapper(root: &Path, f: &syn::ItemFn, baseline: &Value) {
    let mut graph = Graph::load(&root.join("lib.rs"));
    for item in &mut graph.modules.get_mut("crate::market").unwrap().items {
        if matches!(item, Item::Fn(_)) {
            *item = Item::Fn(f.clone())
        }
    }
    assert!(!policy::facade_errors(&graph, "crate::market", "market", baseline).is_empty());
}
fn rejected<T>(action: impl FnOnce() -> T + std::panic::UnwindSafe) -> bool {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(action).is_err();
    std::panic::set_hook(hook);
    result
}
