//! Syntax ownership, not macro expansion or receiver/type dispatch.
use super::*;
use syn::{parse::Parser, punctuated::Punctuated, Token};

pub fn path_text(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|s| s.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}
pub fn item_attrs(item: &Item) -> &[syn::Attribute] {
    match item {
        Item::Fn(x) => &x.attrs,
        Item::Mod(x) => &x.attrs,
        Item::Use(x) => &x.attrs,
        Item::Struct(x) => &x.attrs,
        Item::Enum(x) => &x.attrs,
        Item::Impl(x) => &x.attrs,
        Item::Trait(x) => &x.attrs,
        Item::Const(x) => &x.attrs,
        Item::Type(x) => &x.attrs,
        Item::Static(x) => &x.attrs,
        Item::Macro(x) => &x.attrs,
        _ => &[],
    }
}
// Three-valued cfg evaluation with only `test` fixed. Platform/feature branches
// remain in the syntax inventory; `any(test, feature=...)` is not test-only.
fn cfg_without_test(meta: &syn::Meta) -> Option<bool> {
    match meta {
        syn::Meta::Path(p) if p.is_ident("test") => Some(false),
        syn::Meta::List(list) => {
            let values = list
                .parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
                .ok()?;
            let values = values.iter().map(cfg_without_test).collect::<Vec<_>>();
            if list.path.is_ident("all") {
                if values.contains(&Some(false)) {
                    Some(false)
                } else if values.iter().all(|x| *x == Some(true)) {
                    Some(true)
                } else {
                    None
                }
            } else if list.path.is_ident("any") {
                if values.contains(&Some(true)) {
                    Some(true)
                } else if values.iter().all(|x| *x == Some(false)) {
                    Some(false)
                } else {
                    None
                }
            } else if list.path.is_ident("not") && values.len() == 1 {
                values[0].map(|x| !x)
            } else {
                None
            }
        }
        _ => None,
    }
}
pub fn test_attrs(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|a| {
        a.path().is_ident("cfg")
            && a.parse_args::<syn::Meta>()
                .ok()
                .and_then(|m| cfg_without_test(&m))
                == Some(false)
    })
}

#[derive(Clone, Debug)]
pub struct Binding {
    pub name: String,
    pub path: String,
    pub glob: bool,
}
pub fn bindings(tree: &UseTree) -> Vec<Binding> {
    fn walk(tree: &UseTree, prefix: Vec<String>, out: &mut Vec<Binding>) {
        match tree {
            UseTree::Path(p) => {
                let mut next = prefix;
                next.push(p.ident.to_string());
                walk(&p.tree, next, out)
            }
            UseTree::Group(g) => {
                for tree in &g.items {
                    walk(tree, prefix.clone(), out)
                }
            }
            UseTree::Name(n) => {
                let mut path = prefix;
                let name = if n.ident == "self" {
                    path.last().cloned().unwrap_or_else(|| "self".into())
                } else {
                    path.push(n.ident.to_string());
                    n.ident.to_string()
                };
                out.push(Binding {
                    name,
                    path: path.join("::"),
                    glob: false,
                });
            }
            UseTree::Rename(n) => {
                let mut path = prefix;
                if n.ident != "self" {
                    path.push(n.ident.to_string())
                }
                out.push(Binding {
                    name: n.rename.to_string(),
                    path: path.join("::"),
                    glob: false,
                })
            }
            UseTree::Glob(_) => out.push(Binding {
                name: "*".into(),
                path: prefix.join("::"),
                glob: true,
            }),
        }
    }
    let mut out = Vec::new();
    walk(tree, Vec::new(), &mut out);
    out
}

#[derive(Clone)]
pub struct Module {
    pub file: PathBuf,
    pub items: Vec<Item>,
}
pub struct Graph {
    pub modules: BTreeMap<String, Module>,
    pub symbols: BTreeMap<String, Item>,
}
impl Graph {
    pub fn load(root: &Path) -> Self {
        let root = root
            .canonicalize()
            .unwrap_or_else(|e| panic!("{}: {e}", root.display()));
        let mut graph = Self {
            modules: BTreeMap::new(),
            symbols: BTreeMap::new(),
        };
        let mut active = BTreeSet::new();
        graph.load_file("crate".into(), root, &mut active);
        graph
    }
    fn load_file(&mut self, id: String, file: PathBuf, active: &mut BTreeSet<PathBuf>) {
        assert!(
            active.insert(file.clone()),
            "module file cycle: {}",
            file.display()
        );
        let stem = file.file_stem().unwrap().to_string_lossy();
        let dir = if matches!(stem.as_ref(), "lib" | "main" | "mod") {
            file.parent().unwrap().to_owned()
        } else {
            file.parent().unwrap().join(stem.as_ref())
        };
        self.load_items(id, &file, dir, parse(&file).items, active, false);
        active.remove(&file);
    }
    fn load_items(
        &mut self,
        id: String,
        file: &Path,
        dir: PathBuf,
        items: Vec<Item>,
        active: &mut BTreeSet<PathBuf>,
        inline: bool,
    ) {
        let items = items
            .into_iter()
            .filter(|i| !test_attrs(item_attrs(i)))
            .collect::<Vec<_>>();
        // Platform-alternative declarations share a logical namespace; retain
        // every item, and reject conflicting resolver outcomes rather than last wins.
        self.modules
            .entry(id.clone())
            .or_insert_with(|| Module {
                file: file.to_owned(),
                items: Vec::new(),
            })
            .items
            .extend(items.clone());
        for item in items {
            if let Item::Mod(m) = &item {
                let child = format!("{id}::{}", m.ident);
                if let Some((_, body)) = &m.content {
                    self.load_items(
                        child,
                        file,
                        dir.join(m.ident.to_string()),
                        body.clone(),
                        active,
                        true,
                    )
                } else {
                    let explicit = m.attrs.iter().find_map(|a| {
                        if !a.path().is_ident("path") {
                            return None;
                        }
                        if let syn::Meta::NameValue(v) = &a.meta {
                            if let syn::Expr::Lit(l) = &v.value {
                                if let syn::Lit::Str(s) = &l.lit {
                                    return Some(s.value());
                                }
                            }
                        }
                        None
                    });
                    let path = if let Some(path) = explicit {
                        if inline {
                            dir.join(path)
                        } else {
                            file.parent().unwrap().join(path)
                        }
                    } else {
                        let direct = dir.join(format!("{}.rs", m.ident));
                        let nested = dir.join(m.ident.to_string()).join("mod.rs");
                        assert!(
                            !(direct.exists() && nested.exists()),
                            "ambiguous module {child}"
                        );
                        if direct.exists() {
                            direct
                        } else {
                            nested
                        }
                    };
                    let path = path.canonicalize().unwrap_or_else(|e| {
                        panic!("declared module {child} at {}: {e}", path.display())
                    });
                    self.load_file(child, path, active);
                }
            } else {
                let (kind, name, _) = info(&item);
                if matches!(
                    kind.as_str(),
                    "fn" | "struct" | "enum" | "trait" | "type" | "const" | "static"
                ) {
                    let key = format!("{id}::{name}");
                    if let Some(old) = self.symbols.get(&key) {
                        let guarded =
                            |i: &Item| item_attrs(i).iter().any(|a| a.path().is_ident("cfg"));
                        assert!(
                            guarded(old) && guarded(&item),
                            "duplicate unconditional declaration {key}"
                        );
                        // One canonical identity, all guarded bodies retained in
                        // Module.items. Full cfg/API compatibility stays audited.
                    } else {
                        self.symbols.insert(key, item);
                    }
                }
            }
        }
    }
    pub fn resolve(&self, module: &str, path: &str) -> Option<String> {
        self.resolve_scoped(module, path, &[], &mut BTreeSet::new())
    }
    pub fn resolve_scoped(
        &self,
        module: &str,
        path: &str,
        scopes: &[Vec<Binding>],
        active: &mut BTreeSet<String>,
    ) -> Option<String> {
        let parts = path
            .split("::")
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return None;
        }
        let marker = format!("{module}|{path}");
        if !active.insert(marker.clone()) {
            return None;
        }
        let result = (|| {
            let mut base = module.to_string();
            let mut index = 0;
            match parts[0] {
                "crate" => {
                    base = "crate".into();
                    index = 1
                }
                "self" => index = 1,
                "super" => {
                    while index < parts.len() && parts[index] == "super" {
                        base = base.rsplit_once("::")?.0.to_string();
                        index += 1
                    }
                }
                _ => {
                    for scope in scopes.iter().rev() {
                        if let Some(b) = scope.iter().find(|b| !b.glob && b.name == parts[0]) {
                            let target = self.resolve_scoped(module, &b.path, scopes, active)?;
                            return self.walk(target, &parts[1..], active, true);
                        }
                        let candidates = scope
                            .iter()
                            .filter(|b| b.glob)
                            .filter_map(|b| {
                                let target =
                                    self.resolve_scoped(module, &b.path, scopes, active)?;
                                self.walk(target, &parts, active, false)
                            })
                            .collect::<BTreeSet<_>>();
                        assert!(
                            candidates.len() <= 1,
                            "ambiguous scoped import {module}: {path}"
                        );
                        if let Some(target) = candidates.into_iter().next() {
                            return Some(target);
                        }
                    }
                }
            }
            self.walk(base.clone(), &parts[index..], active, index > 0)
                .or_else(|| {
                    if index == 0 && matches!(parts[0], "std" | "core" | "gp_core" | "gp_research")
                    {
                        Some(path.into())
                    } else {
                        None
                    }
                })
        })();
        active.remove(&marker);
        result
    }
    fn walk(
        &self,
        mut target: String,
        parts: &[&str],
        active: &mut BTreeSet<String>,
        mut established_owner: bool,
    ) -> Option<String> {
        let marker = format!("walk:{target}|{}", parts.join("::"));
        if !active.insert(marker.clone()) {
            return None;
        }
        let result = (|| {
            for (index, part) in parts.iter().enumerate() {
                if !target.starts_with("crate::") && target != "crate" {
                    return Some(format!("{target}::{}", parts[index..].join("::")));
                }
                if self.symbols.contains_key(&target) && !self.modules.contains_key(&target) {
                    if self.symbols.get(&target).is_some_and(|item| {
                        matches!(item, Item::Fn(_) | Item::Const(_) | Item::Static(_))
                    }) {
                        return None;
                    }
                    return Some(format!("{target}::{}", parts[index..].join("::")));
                }
                let module = match self.modules.get(&target) {
                    Some(node) => node,
                    None => {
                        if self
                            .known_owner(&target)
                            .is_some_and(|owner| owner != "crate")
                        {
                            return Some(format!("{target}::{}", parts[index..].join("::")));
                        }
                        return None;
                    }
                };
                let direct = format!("{target}::{part}");
                let type_prefix = index + 1 < parts.len();
                let eligible_symbol = self.symbols.get(&direct).is_some_and(|item| {
                    !type_prefix
                        || matches!(
                            item,
                            Item::Struct(_) | Item::Enum(_) | Item::Type(_) | Item::Trait(_)
                        )
                });
                if self.modules.contains_key(&direct) || eligible_symbol {
                    target = direct;
                    established_owner = true;
                    continue;
                }
                let candidates = module
                    .items
                    .iter()
                    .filter_map(|i| {
                        if let Item::Use(u) = i {
                            Some(bindings(&u.tree))
                        } else {
                            None
                        }
                    })
                    .flatten()
                    .filter_map(|b| {
                        if b.glob {
                            let base = self.resolve_scoped(&target, &b.path, &[], active)?;
                            self.walk(base, &[*part], active, false)
                        } else if b.name == *part {
                            self.resolve_scoped(&target, &b.path, &[], active)
                        } else {
                            None
                        }
                    })
                    .collect::<BTreeSet<_>>();
                assert!(
                    candidates.len() <= 1,
                    "ambiguous owned alias {target}::{part}: {candidates:?}"
                );
                match candidates.into_iter().next() {
                    Some(next) => {
                        target = next;
                        established_owner = true;
                    }
                    None => {
                        if established_owner && target != "crate" {
                            return Some(format!("{target}::{}", parts[index..].join("::")));
                        }
                        return None;
                    }
                }
            }
            Some(target)
        })();
        active.remove(&marker);
        result
    }
    pub fn known_owner<'a>(&self, path: &'a str) -> Option<&'a str> {
        let mut owner = path;
        loop {
            if self.modules.contains_key(owner) || self.symbols.contains_key(owner) {
                return Some(owner);
            }
            owner = owner.rsplit_once("::")?.0;
        }
    }
    pub fn exports(&self, module: &str, active: &mut BTreeSet<String>) -> BTreeMap<String, String> {
        assert!(
            active.insert(module.into()),
            "public reexport cycle at {module}"
        );
        let mut out = BTreeMap::new();
        for item in &self.modules[module].items {
            let (kind, name, is_public) = info(item);
            if is_public
                && matches!(
                    kind.as_str(),
                    "fn" | "struct" | "enum" | "type" | "trait" | "const" | "static"
                )
            {
                insert_export(&mut out, name, format!("{module}::{}", info(item).1));
            }
            if let Item::Use(u) = item {
                if public(&u.vis) {
                    for b in bindings(&u.tree) {
                        let target = self.resolve(module, &b.path).unwrap_or_else(|| {
                            panic!("unresolved public export {module}: {}", b.path)
                        });
                        if b.glob {
                            for (name, target) in self.exports(&target, active) {
                                insert_export(&mut out, name, target)
                            }
                        } else {
                            assert!(
                                self.symbols.contains_key(&target)
                                    || self.modules.contains_key(&target),
                                "export target missing {target}"
                            );
                            insert_export(&mut out, b.name, target)
                        }
                    }
                }
            }
        }
        active.remove(module);
        out
    }
    pub fn signature_exports(&self) -> BTreeMap<String, String> {
        self.exports("crate", &mut BTreeSet::new())
            .into_iter()
            .filter_map(|(name, target)| {
                let mut item = self.symbols.get(&target)?.clone();
                let ident = syn::Ident::new(&name, item.span());
                match &mut item {
                    Item::Fn(x) => x.sig.ident = ident,
                    Item::Struct(x) => x.ident = ident,
                    Item::Enum(x) => x.ident = ident,
                    Item::Type(x) => x.ident = ident,
                    Item::Trait(x) => x.ident = ident,
                    Item::Const(x) => x.ident = ident,
                    Item::Static(x) => x.ident = ident,
                    _ => {}
                }
                Some((name, signature(&item)))
            })
            .collect()
    }
    pub fn methods(&self) -> BTreeMap<String, String> {
        let exported = self.exports("crate", &mut BTreeSet::new());
        let mut out = BTreeMap::new();
        for (module, node) in &self.modules {
            for item in &node.items {
                if let Item::Impl(i) = item {
                    if i.trait_.is_some() {
                        continue;
                    }
                    let syn::Type::Path(t) = i.self_ty.as_ref() else {
                        continue;
                    };
                    let Some(owner) = self.resolve(module, &path_text(&t.path)) else {
                        continue;
                    };
                    for (exposed, target) in &exported {
                        if target == &owner {
                            for member in &i.items {
                                if let syn::ImplItem::Fn(m) = member {
                                    if public(&m.vis) && !test_attrs(&m.attrs) {
                                        let f = Item::Fn(
                                            syn::parse2(m.to_token_stream())
                                                .expect("inherent method"),
                                        );
                                        let key = format!("{exposed}::{}", m.sig.ident);
                                        let sig = signature(&f);
                                        if let Some(old) = out.insert(key.clone(), sig.clone()) {
                                            assert_eq!(old, sig, "conflicting method {key}")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        out
    }
}
fn insert_export(out: &mut BTreeMap<String, String>, name: String, target: String) {
    if let Some(old) = out.insert(name.clone(), target.clone()) {
        assert_eq!(old, target, "conflicting public export {name}")
    }
}

pub fn macro_expressions(tokens: TokenStream) -> Vec<syn::Expr> {
    if let Ok(args) = Punctuated::<syn::Expr, Token![,]>::parse_terminated.parse2(tokens.clone()) {
        return args.into_iter().collect();
    }
    // Declarative macros may have non-expression punctuation. Inspect literal
    // token paths/calls and recurse groups; never parse string/comment contents.
    let values = tokens.into_iter().collect::<Vec<_>>();
    let mut out = Vec::new();
    let mut index = 0;
    while index < values.len() {
        if let TokenTree::Group(g) = &values[index] {
            out.extend(macro_expressions(g.stream()));
            index += 1;
            continue;
        }
        if let TokenTree::Ident(_) = &values[index] {
            let mut end = index + 1;
            while end + 2 < values.len()
                && matches!(&values[end],TokenTree::Punct(p) if p.as_char()==':')
                && matches!(&values[end+1],TokenTree::Punct(p) if p.as_char()==':')
            {
                if let TokenTree::Ident(_) = &values[end + 2] {
                    end += 3
                } else {
                    break;
                }
            }
            let call = matches!(values.get(end),Some(TokenTree::Group(g)) if g.delimiter()==proc_macro2::Delimiter::Parenthesis);
            if end > index + 1 || call {
                if let Ok(expr) = syn::parse2(values[index..end].iter().cloned().collect()) {
                    out.push(expr)
                }
            }
            index = end;
            continue;
        }
        index += 1
    }
    out
}

// One ordered lexical stack is shared by references and captured calls.
#[derive(Clone, Default)]
struct LexicalScope {
    imports: Vec<Binding>,
    values: BTreeSet<String>,
    types: BTreeSet<String>,
}
impl LexicalScope {
    fn generics(generics: &syn::Generics) -> Self {
        let mut scope = Self::default();
        for parameter in &generics.params {
            match parameter {
                syn::GenericParam::Type(t) => {
                    scope.types.insert(t.ident.to_string());
                }
                syn::GenericParam::Const(c) => {
                    scope.values.insert(c.ident.to_string());
                }
                _ => {}
            }
        }
        scope
    }
    fn function(signature: &syn::Signature) -> Self {
        let mut scope = Self::generics(&signature.generics);
        for arg in &signature.inputs {
            if let syn::FnArg::Typed(arg) = arg {
                pattern_names(&arg.pat, &mut scope.values)
            }
        }
        scope
    }
    fn pattern(pattern: &syn::Pat) -> Self {
        let mut scope = Self::default();
        pattern_names(pattern, &mut scope.values);
        scope
    }
    fn block(block: &syn::Block) -> Self {
        let mut scope = Self::default();
        for statement in &block.stmts {
            if let syn::Stmt::Item(item) = statement {
                if test_attrs(item_attrs(item)) {
                    continue;
                }
                match item {
                    Item::Use(u) => scope.imports.extend(bindings(&u.tree)),
                    Item::Fn(f) => {
                        scope.values.insert(f.sig.ident.to_string());
                    }
                    Item::Const(c) => {
                        scope.values.insert(c.ident.to_string());
                    }
                    Item::Static(s) => {
                        scope.values.insert(s.ident.to_string());
                    }
                    Item::Struct(s) => {
                        scope.types.insert(s.ident.to_string());
                        if !matches!(s.fields, syn::Fields::Named(_)) {
                            scope.values.insert(s.ident.to_string());
                        }
                    }
                    Item::Enum(e) => {
                        scope.types.insert(e.ident.to_string());
                    }
                    Item::Type(t) => {
                        scope.types.insert(t.ident.to_string());
                    }
                    Item::Trait(t) => {
                        scope.types.insert(t.ident.to_string());
                    }
                    Item::Mod(m) => {
                        scope.types.insert(m.ident.to_string());
                    }
                    _ => {}
                }
            }
        }
        scope
    }
}
fn import_namespace(graph: &Graph, target: &str, want_type: bool) -> bool {
    let module = graph.modules.contains_key(target);
    let symbol = graph.symbols.get(target);
    match (want_type, symbol) {
        (true, Some(Item::Struct(_) | Item::Enum(_) | Item::Type(_) | Item::Trait(_))) => true,
        (false, Some(Item::Fn(_) | Item::Const(_) | Item::Static(_))) => true,
        (false, Some(Item::Struct(s))) => !matches!(s.fields, syn::Fields::Named(_)),
        (true, _) if module => true,
        (_, None) if !module => true, // external/generated namespace remains unknown
        _ => false,
    }
}
fn resolve_lexical(
    graph: &Graph,
    module: &str,
    raw: &str,
    value: bool,
    scopes: &[LexicalScope],
    active: &mut BTreeSet<String>,
) -> Option<String> {
    let parts = raw
        .split("::")
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let first = *parts.first()?;
    if matches!(first, "crate" | "self" | "super") {
        return graph.resolve(module, raw);
    }
    let want_type = !value || parts.len() > 1;
    for (scope_index, scope) in scopes.iter().enumerate().rev() {
        let shadows = if want_type {
            &scope.types
        } else {
            &scope.values
        };
        if shadows.contains(first) {
            return None;
        }
        let mut named = BTreeSet::new();
        let mut unresolved = false;
        for (binding_index, binding) in scope
            .imports
            .iter()
            .enumerate()
            .filter(|(_, b)| !b.glob && b.name == first)
        {
            let marker =
                format!("lexical-import:{module}:{scope_index}:{binding_index}:{want_type}");
            if !active.insert(marker.clone()) {
                continue;
            }
            let target = resolve_lexical(graph, module, &binding.path, !want_type, scopes, active);
            active.remove(&marker);
            match target {
                Some(target) if import_namespace(graph, &target, want_type) => {
                    named.insert(target);
                }
                None => unresolved = true,
                _ => {}
            }
        }
        assert!(named.len() <= 1, "ambiguous lexical import {module}: {raw}");
        if let Some(target) = named.into_iter().next() {
            return graph.walk(target, &parts[1..], active, true);
        }
        if unresolved {
            return None;
        }
        let mut globbed = BTreeSet::new();
        for (binding_index, binding) in scope.imports.iter().enumerate().filter(|(_, b)| b.glob) {
            let marker = format!("lexical-glob:{module}:{scope_index}:{binding_index}:{want_type}");
            if !active.insert(marker.clone()) {
                continue;
            }
            if let Some(owner) =
                resolve_lexical(graph, module, &binding.path, false, scopes, active)
            {
                if let Some(target) = graph.walk(owner, &parts, active, false) {
                    if import_namespace(graph, &target, want_type) {
                        globbed.insert(target);
                    }
                }
            }
            active.remove(&marker);
        }
        assert!(globbed.len() <= 1, "ambiguous lexical glob {module}: {raw}");
        if let Some(target) = globbed.into_iter().next() {
            return Some(target);
        }
    }
    graph.resolve(module, raw)
}
fn resolve_import_targets(
    graph: &Graph,
    module: &str,
    raw: &str,
    scopes: &[LexicalScope],
) -> BTreeSet<String> {
    // `use` names items, not an expression value. Preserve each valid namespace
    // (module/type or function/value) without inheriting a later call's context.
    [false, true]
        .into_iter()
        .filter_map(|value| {
            resolve_lexical(graph, module, raw, value, scopes, &mut BTreeSet::new())
        })
        .collect()
}
pub struct References<'g> {
    graph: &'g Graph,
    module: &'g str,
    lexical: Vec<LexicalScope>,
    pub paths: BTreeMap<String, BTreeSet<String>>,
    caller: Option<String>,
    impl_owner: Option<String>,
    pub records: Vec<Reference>,
    pub calls: Vec<ResolvedCall>,
}
#[derive(Clone)]
pub struct Reference {
    pub target: String,
    pub raw: String,
    pub caller: Option<String>,
    pub line: usize,
    pub import: bool,
}
#[derive(Clone)]
pub struct ResolvedCall {
    pub target: String,
    pub caller: Option<String>,
    pub expression: syn::ExprCall,
    lexical: Vec<LexicalScope>,
}
impl ResolvedCall {
    pub fn resolve_name(&self, graph: &Graph, module: &str, name: &str) -> Option<String> {
        resolve_lexical(
            graph,
            module,
            name,
            true,
            &self.lexical,
            &mut BTreeSet::new(),
        )
    }
}
impl<'g> References<'g> {
    pub fn collect(graph: &'g Graph, module: &'g str) -> BTreeMap<String, BTreeSet<String>> {
        Self::scan(graph, module).paths
    }
    pub fn records(graph: &'g Graph, module: &'g str) -> Vec<Reference> {
        Self::scan(graph, module).records
    }
    pub fn calls(graph: &'g Graph, module: &'g str) -> Vec<ResolvedCall> {
        Self::scan(graph, module).calls
    }
    fn scan(graph: &'g Graph, module: &'g str) -> Self {
        let mut visitor = Self {
            graph,
            module,
            lexical: Vec::new(),
            paths: BTreeMap::new(),
            caller: None,
            impl_owner: None,
            records: Vec::new(),
            calls: Vec::new(),
        };
        for item in &graph.modules[module].items {
            visitor.visit_item(item)
        }
        visitor
    }
    fn resolve(&self, raw: &str, value: bool) -> Option<String> {
        resolve_lexical(
            self.graph,
            self.module,
            raw,
            value,
            &self.lexical,
            &mut BTreeSet::new(),
        )
    }
    fn add(&mut self, path: &syn::Path, value: bool) {
        let raw = path_text(path);
        if let Some(target) = self.resolve(&raw, value) {
            self.records.push(Reference {
                target: target.clone(),
                raw: raw.clone(),
                caller: self.caller.clone(),
                line: path.span().start().line,
                import: false,
            });
            self.paths.entry(target).or_default().insert(raw);
        }
    }
}
fn pattern_names(pattern: &syn::Pat, out: &mut BTreeSet<String>) {
    struct Names<'a>(&'a mut BTreeSet<String>);
    impl<'ast> Visit<'ast> for Names<'_> {
        fn visit_pat_ident(&mut self, p: &'ast syn::PatIdent) {
            self.0.insert(p.ident.to_string());
            syn::visit::visit_pat_ident(self, p)
        }
    }
    Names(out).visit_pat(pattern)
}
impl<'ast> Visit<'ast> for References<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if !test_attrs(item_attrs(item)) {
            syn::visit::visit_item(self, item)
        }
    }
    fn visit_item_mod(&mut self, _: &'ast syn::ItemMod) {} // declared children have their own traversal
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_item_struct(self, item);
        self.lexical.pop();
    }
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_item_enum(self, item);
        self.lexical.pop();
    }
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_item_type(self, item);
        self.lexical.pop();
    }
    fn visit_item_trait(&mut self, item: &'ast syn::ItemTrait) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_item_trait(self, item);
        self.lexical.pop();
    }
    fn visit_trait_item_fn(&mut self, item: &'ast syn::TraitItemFn) {
        let prior = self
            .caller
            .replace(format!("{}::<trait>::{}", self.module, item.sig.ident));
        self.lexical.push(LexicalScope::function(&item.sig));
        syn::visit::visit_trait_item_fn(self, item);
        self.lexical.pop();
        self.caller = prior;
    }
    fn visit_trait_item_type(&mut self, item: &'ast syn::TraitItemType) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_trait_item_type(self, item);
        self.lexical.pop();
    }
    fn visit_impl_item_type(&mut self, item: &'ast syn::ImplItemType) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        syn::visit::visit_impl_item_type(self, item);
        self.lexical.pop();
    }
    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        for binding in bindings(&item.tree) {
            for target in
                resolve_import_targets(self.graph, self.module, &binding.path, &self.lexical)
            {
                self.records.push(Reference {
                    target: target.clone(),
                    raw: binding.path.clone(),
                    caller: self.caller.clone(),
                    line: item.span().start().line,
                    import: true,
                });
                self.paths
                    .entry(target)
                    .or_default()
                    .insert(binding.path.clone());
            }
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let caller = format!(
            "{}::{}",
            self.caller.as_deref().unwrap_or(self.module),
            item.sig.ident
        );
        let prior = self.caller.replace(caller);
        self.lexical.push(LexicalScope::function(&item.sig));
        syn::visit::visit_item_fn(self, item);
        self.lexical.pop();
        self.caller = prior;
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.lexical.push(LexicalScope::generics(&item.generics));
        let prior = self.impl_owner.take();
        self.impl_owner = if let syn::Type::Path(path) = item.self_ty.as_ref() {
            self.resolve(&path_text(&path.path), false)
        } else {
            None
        };
        syn::visit::visit_item_impl(self, item);
        self.impl_owner = prior;
        self.lexical.pop();
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if test_attrs(&item.attrs) {
            return;
        }
        let caller = format!(
            "{}::{}",
            self.impl_owner.as_deref().unwrap_or("<impl>"),
            item.sig.ident
        );
        let prior = self.caller.replace(caller);
        self.lexical.push(LexicalScope::function(&item.sig));
        syn::visit::visit_impl_item_fn(self, item);
        self.lexical.pop();
        self.caller = prior;
    }
    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.lexical.push(LexicalScope::block(block));
        syn::visit::visit_block(self, block);
        self.lexical.pop();
    }
    fn visit_local(&mut self, item: &'ast syn::Local) {
        syn::visit::visit_local(self, item);
        if let Some(scope) = self.lexical.last_mut() {
            pattern_names(&item.pat, &mut scope.values)
        }
    }
    fn visit_expr_closure(&mut self, item: &'ast syn::ExprClosure) {
        let mut scope = LexicalScope::default();
        for pattern in &item.inputs {
            pattern_names(pattern, &mut scope.values)
        }
        self.lexical.push(scope);
        syn::visit::visit_expr_closure(self, item);
        self.lexical.pop();
    }
    fn visit_expr_path(&mut self, item: &'ast syn::ExprPath) {
        if let Some(qself) = &item.qself {
            self.visit_type(&qself.ty)
        }
        self.add(&item.path, true);
        for segment in &item.path.segments {
            self.visit_path_arguments(&segment.arguments)
        }
    }
    fn visit_type_path(&mut self, item: &'ast syn::TypePath) {
        self.add(&item.path, false);
        syn::visit::visit_type_path(self, item)
    }
    fn visit_path(&mut self, path: &'ast syn::Path) {
        self.add(path, false);
        syn::visit::visit_path(self, path)
    }
    fn visit_expr_call(&mut self, item: &'ast syn::ExprCall) {
        if let syn::Expr::Path(path) = item.func.as_ref() {
            if path.qself.is_none() {
                if let Some(target) = self.resolve(&path_text(&path.path), true) {
                    self.calls.push(ResolvedCall {
                        target,
                        caller: self.caller.clone(),
                        expression: item.clone(),
                        lexical: self.lexical.clone(),
                    });
                }
            }
        }
        syn::visit::visit_expr_call(self, item)
    }
    fn visit_expr_for_loop(&mut self, item: &'ast syn::ExprForLoop) {
        for attr in &item.attrs {
            self.visit_attribute(attr)
        }
        self.visit_expr(&item.expr);
        self.visit_pat(&item.pat);
        self.lexical.push(LexicalScope::pattern(&item.pat));
        self.visit_block(&item.body);
        self.lexical.pop();
    }
    fn visit_expr_if(&mut self, item: &'ast syn::ExprIf) {
        for attr in &item.attrs {
            self.visit_attribute(attr)
        }
        self.visit_expr(&item.cond);
        let scope = if let syn::Expr::Let(condition) = item.cond.as_ref() {
            LexicalScope::pattern(&condition.pat)
        } else {
            LexicalScope::default()
        };
        self.lexical.push(scope);
        self.visit_block(&item.then_branch);
        self.lexical.pop();
        if let Some((_, branch)) = &item.else_branch {
            self.visit_expr(branch)
        }
    }
    fn visit_expr_while(&mut self, item: &'ast syn::ExprWhile) {
        for attr in &item.attrs {
            self.visit_attribute(attr)
        }
        self.visit_expr(&item.cond);
        let scope = if let syn::Expr::Let(condition) = item.cond.as_ref() {
            LexicalScope::pattern(&condition.pat)
        } else {
            LexicalScope::default()
        };
        self.lexical.push(scope);
        self.visit_block(&item.body);
        self.lexical.pop();
    }
    fn visit_arm(&mut self, item: &'ast syn::Arm) {
        for attr in &item.attrs {
            self.visit_attribute(attr)
        }
        let (pattern, guard) = if let syn::Pat::Guard(pattern) = &item.pat {
            for attr in &pattern.attrs {
                self.visit_attribute(attr)
            }
            (pattern.pat.as_ref(), Some(pattern.guard.as_ref()))
        } else {
            (&item.pat, None)
        };
        self.visit_pat(pattern);
        self.lexical.push(LexicalScope::pattern(pattern));
        if let Some(guard) = guard {
            self.visit_expr(guard)
        }
        self.visit_expr(&item.body);
        self.lexical.pop();
    }
    fn visit_macro(&mut self, item: &'ast syn::Macro) {
        for expr in macro_expressions(item.tokens.clone()) {
            self.visit_expr(&expr)
        }
    }
}
