/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::path;

use syn;

use bindgen::cargo::Cargo;
use bindgen::config::Config;
use bindgen::ir::{AnnotationSet, Cfg, Constant, Documentation, Enum, Function};
use bindgen::ir::{ItemMap, OpaqueItem, Specialization, Static, Struct, Typedef, Union};
use bindgen::library::Library;
use bindgen::rust_lib;
use bindgen::utilities::{SynAbiHelpers, SynItemHelpers};

#[derive(Debug, Clone)]
pub struct LibraryBuilder {
    config: Config,
    srcs: Vec<path::PathBuf>,
    lib: Option<Cargo>,
    std_types: bool,
}

impl LibraryBuilder {
    pub fn new() -> LibraryBuilder {
        LibraryBuilder {
            config: Config::default(),
            srcs: Vec::new(),
            lib: None,
            std_types: false,
        }
    }

    pub fn with_config(mut self, config: Config) -> LibraryBuilder {
        self.config = config;
        self
    }

    pub fn with_std_types(mut self) -> LibraryBuilder {
        self.std_types = true;
        self
    }

    pub fn with_src(mut self, src: &path::Path) -> LibraryBuilder {
        self.srcs.push(src.to_owned());
        self
    }

    pub fn with_crate(mut self, lib: Cargo) -> LibraryBuilder {
        debug_assert!(self.lib.is_none());
        self.lib = Some(lib);
        self
    }

    pub fn build(self) -> Result<Library, String> {
        let mut result = LibraryParseResult::new();

        if self.std_types {
            result.add_std_types();
        }

        for x in &self.srcs {
            rust_lib::parse_src(x, &mut |crate_name, items| {
                result.load_syn_crate_mod("", &crate_name, &None, items);
            })?;
        }

        if let Some(x) = self.lib.clone() {
            rust_lib::parse_lib(x,
                                self.config.parse.parse_deps,
                                &self.config.parse.include,
                                &self.config.parse.exclude,
                                &self.config.parse.expand,
                                &mut |binding_crate_name, crate_name, mod_cfg, items| {
                result.load_syn_crate_mod(binding_crate_name, &crate_name, &mod_cfg, items);
            })?;
        }

        result.functions.sort_by(|x, y| x.name.cmp(&y.name));

        Ok(Library::new(self.config,
                        result.constants,
                        result.globals,
                        result.enums,
                        result.structs,
                        result.unions,
                        result.opaque_items,
                        result.typedefs,
                        result.specializations,
                        result.functions))
    }
}

#[derive(Debug, Clone)]
struct LibraryParseResult {
    constants: ItemMap<Constant>,
    globals: ItemMap<Static>,
    enums: ItemMap<Enum>,
    structs: ItemMap<Struct>,
    unions: ItemMap<Union>,
    opaque_items: ItemMap<OpaqueItem>,
    typedefs: ItemMap<Typedef>,
    specializations: ItemMap<Specialization>,
    functions: Vec<Function>,
}

impl LibraryParseResult {
    fn new() -> LibraryParseResult {
        LibraryParseResult {
            enums: ItemMap::new(),
            constants: ItemMap::new(),
            globals: ItemMap::new(),
            structs: ItemMap::new(),
            unions: ItemMap::new(),
            opaque_items: ItemMap::new(),
            typedefs: ItemMap::new(),
            specializations: ItemMap::new(),
            functions: Vec::new(),
        }
    }

    fn add_std_types(&mut self) {
        {
            let mut add_opaque = |name: &str, generic_params: Vec<&str>| {
                self.opaque_items.try_insert(OpaqueItem {
                    name: name.to_owned(),
                    generic_params: generic_params.iter()
                                                  .map(|x| (*x).to_owned())
                                                  .collect(),
                    cfg: None,
                    annotations: AnnotationSet::new(),
                    documentation: Documentation::none(),
                })
            };

            add_opaque("String", vec![]);
            add_opaque("Box", vec!["T"]);
            add_opaque("Rc", vec!["T"]);
            add_opaque("Arc", vec!["T"]);
            add_opaque("Result", vec!["T", "E"]);
            add_opaque("Option", vec!["T"]);
            add_opaque("Vec", vec!["T"]);
            add_opaque("HashMap", vec!["K", "V"]);
            add_opaque("BTreeMap", vec!["K", "V"]);
            add_opaque("HashSet", vec!["T"]);
            add_opaque("BTreeSet", vec!["T"]);
            add_opaque("LinkedList", vec!["T"]);
            add_opaque("VecDeque", vec!["T"]);
        }
    }

    fn load_syn_crate_mod(&mut self,
                          binding_crate_name: &str,
                          crate_name: &str,
                          mod_cfg: &Option<Cfg>,
                          items: &Vec<syn::Item>) {
        for item in items {
            match item.node {
                syn::ItemKind::ForeignMod(ref block) => {
                    self.load_syn_foreign_mod(binding_crate_name,
                                              crate_name,
                                              mod_cfg,
                                              item,
                                              block);
                }
                syn::ItemKind::Fn(ref decl,
                                  ref _unsafe,
                                  ref _const,
                                  ref abi,
                                  ref _generic,
                                  ref _block) => {
                    self.load_syn_fn(binding_crate_name,
                                     crate_name,
                                     mod_cfg,
                                     item,
                                     decl,
                                     abi);
                }
                syn::ItemKind::Const(ref ty, ref expr) => {
                    self.load_syn_const(binding_crate_name,
                                        crate_name,
                                        mod_cfg,
                                        item,
                                        ty,
                                        expr);
                }
                syn::ItemKind::Static(ref ty, ref mutability, ref _expr) => {
                    self.load_syn_static(binding_crate_name,
                                         crate_name,
                                         mod_cfg,
                                         item,
                                         ty,
                                         mutability);
                }
                syn::ItemKind::Struct(ref variant, ref generics) => {
                    self.load_syn_struct(crate_name, mod_cfg, item, variant, generics);
                }
                syn::ItemKind::Union(ref variant, ref generics) => {
                    self.load_syn_union(crate_name, mod_cfg, item, variant, generics);
                }
                syn::ItemKind::Enum(ref variants, ref generics) => {
                    self.load_syn_enum(crate_name, mod_cfg, item, variants, generics);
                }
                syn::ItemKind::Ty(ref ty, ref generics) => {
                    self.load_syn_ty(crate_name, mod_cfg, item, ty, generics);
                }
                _ => { }
            }
        }
    }

    /// Enters a `extern "C" { }` declaration and loads function declarations.
    fn load_syn_foreign_mod(&mut self,
                            binding_crate_name: &str,
                            crate_name: &str,
                            mod_cfg: &Option<Cfg>,
                            item: &syn::Item,
                            block: &syn::ForeignMod) {
        if !block.abi.is_c() {
            info!("Skip {}::{} - (extern block must be extern C).", crate_name, &item.ident);
            return;
        }

        for foreign_item in &block.items {
            match foreign_item.node {
                syn::ForeignItemKind::Fn(ref decl,
                                         ref _generic) => {
                    if crate_name != binding_crate_name {
                        info!("Skip {}::{} - (fn's outside of the binding crate are not used).",
                              crate_name,
                              &foreign_item.ident);
                        return;
                    }

                    match Function::load(foreign_item.ident.to_string(),
                                         decl,
                                         true,
                                         &foreign_item.attrs,
                                         mod_cfg) {
                        Ok(func) => {
                            info!("Take {}::{}.", crate_name, &foreign_item.ident);

                            self.functions.push(func);
                        }
                        Err(msg) => {
                            error!("Cannot use fn {}::{} ({}).",
                                   crate_name,
                                   &foreign_item.ident,
                                   msg);
                        },
                    }
                }
                _ => {}
            }
        }
    }

    /// Loads a `fn` declaration
    fn load_syn_fn(&mut self,
                   binding_crate_name: &str,
                   crate_name: &str,
                   mod_cfg: &Option<Cfg>,
                   item: &syn::Item,
                   decl: &syn::FnDecl,
                   abi: &Option<syn::Abi>) {
        if crate_name != binding_crate_name {
            info!("Skip {}::{} - (fn's outside of the binding crate are not used).",
                  crate_name,
                  &item.ident);
            return;
        }

        if item.is_no_mangle() && (abi.is_omitted() || abi.is_c()) {
            match Function::load(item.ident.to_string(),
                                 decl,
                                 false,
                                 &item.attrs,
                                 mod_cfg) {
                Ok(func) => {
                    info!("Take {}::{}.", crate_name, &item.ident);

                    self.functions.push(func);
                }
                Err(msg) => {
                    error!("Cannot use fn {}::{} ({}).",
                           crate_name,
                           &item.ident,
                           msg);
                },
            }
        } else {
            if (abi.is_omitted() || abi.is_c()) && !item.is_no_mangle() {
                warn!("Skip {}::{} - (`extern` but not `no_mangle`).",
                      crate_name,
                      &item.ident);
            }
            if abi.is_some() && !(abi.is_omitted() || abi.is_c()) {
                warn!("Skip {}::{} - (non `extern \"C\"`).",
                      crate_name,
                      &item.ident);
            }
        }
    }

    /// Loads a `const` declaration
    fn load_syn_const(&mut self,
                      binding_crate_name: &str,
                      crate_name: &str,
                      mod_cfg: &Option<Cfg>,
                      item: &syn::Item,
                      ty: &syn::Ty,
                      expr: &syn::Expr) {
        if crate_name != binding_crate_name {
            info!("Skip {}::{} - (const's outside of the binding crate are not used).",
                  crate_name,
                  &item.ident);
            return;
        }

        let const_name = item.ident.to_string();

        match Constant::load(const_name.clone(),
                             ty,
                             expr,
                             &item.attrs,
                             mod_cfg) {
            Ok(constant) => {
                info!("Take {}::{}.", crate_name, &item.ident);

                self.constants.try_insert(constant);
            }
            Err(msg) => {
                warn!("Skip {}::{} - ({})",
                      crate_name,
                      &item.ident,
                      msg);
            }
        }
    }

    /// Loads a `static` declaration
    fn load_syn_static(&mut self,
                       binding_crate_name: &str,
                       crate_name: &str,
                       mod_cfg: &Option<Cfg>,
                       item: &syn::Item,
                       ty: &syn::Ty,
                       mutability: &syn::Mutability) {
        if crate_name != binding_crate_name {
            info!("Skip {}::{} - (static's outside of the binding crate are not used).",
                  crate_name,
                  &item.ident);
            return;
        }

        let static_name = item.ident.to_string();

        match Static::load(static_name.clone(),
                           ty,
                           mutability,
                           &item.attrs,
                            mod_cfg) {
            Ok(constant) => {
                info!("Take {}::{}.", crate_name, &item.ident);

                self.globals.try_insert(constant);
            }
            Err(msg) => {
                warn!("Skip {}::{} - ({})",
                      crate_name,
                      &item.ident,
                      msg);
            }
        }
    }

    /// Loads a `struct` declaration
    fn load_syn_struct(&mut self,
                       crate_name: &str,
                       mod_cfg: &Option<Cfg>,
                       item: &syn::Item,
                       variant: &syn::VariantData,
                       generics: &syn::Generics) {
        let struct_name = item.ident.to_string();

        match Struct::load(struct_name.clone(),
                           variant,
                           generics,
                           &item.attrs,
                           mod_cfg) {
            Ok(st) => {
                info!("Take {}::{}.", crate_name, &item.ident);

                self.structs.try_insert(st);
            }
            Err(msg) => {
                info!("Take {}::{} - opaque ({}).",
                      crate_name,
                      &item.ident,
                      msg);
                self.opaque_items.try_insert(OpaqueItem::new(struct_name,
                                                             generics,
                                                             &item.attrs,
                                                             mod_cfg));
            }
        }
    }

    /// Loads a `union` declaration
    fn load_syn_union(&mut self,
                      crate_name: &str,
                      mod_cfg: &Option<Cfg>,
                      item: &syn::Item,
                      variant: &syn::VariantData,
                      generics: &syn::Generics) {
        let union_name = item.ident.to_string();

        match Union::load(union_name.clone(),
                          variant,
                          generics,
                          &item.attrs,
                          mod_cfg) {
            Ok(st) => {
                info!("Take {}::{}.", crate_name, &item.ident);

                self.unions.try_insert(st);
            }
            Err(msg) => {
                info!("Take {}::{} - opaque ({}).",
                      crate_name,
                      &item.ident,
                      msg);
                self.opaque_items.try_insert(OpaqueItem::new(union_name,
                                                             generics,
                                                             &item.attrs,
                                                             mod_cfg));
            }
        }
    }

    /// Loads a `enum` declaration
    fn load_syn_enum(&mut self,
                     crate_name: &str,
                     mod_cfg: &Option<Cfg>,
                     item: &syn::Item,
                     variants: &Vec<syn::Variant>,
                     generics: &syn::Generics) {
        if !generics.lifetimes.is_empty() ||
           !generics.ty_params.is_empty() ||
           !generics.where_clause.predicates.is_empty() {
            info!("Skip {}::{} - (has generics or lifetimes or where bounds).",
                  crate_name,
                  &item.ident);
            return;
        }
        let enum_name = item.ident.to_string();

        match Enum::load(enum_name.clone(),
                         variants,
                         &item.attrs,
                         mod_cfg) {
            Ok(en) => {
                info!("Take {}::{}.", crate_name, &item.ident);
                self.enums.try_insert(en);
            }
            Err(msg) => {
                info!("Take {}::{} - opaque ({}).", crate_name, &item.ident, msg);
                self.opaque_items.try_insert(OpaqueItem::new(enum_name,
                                                             generics,
                                                             &item.attrs,
                                                             mod_cfg));
            }
        }
    }

    /// Loads a `type` declaration
    fn load_syn_ty(&mut self,
                   crate_name: &str,
                   mod_cfg: &Option<Cfg>,
                   item: &syn::Item,
                   ty: &syn::Ty,
                   generics: &syn::Generics) {
        let alias_name = item.ident.to_string();

        let fail1 = if generics.lifetimes.is_empty() &&
                       generics.ty_params.is_empty()
        {
            match Typedef::load(alias_name.clone(),
                                ty,
                                &item.attrs,
                                mod_cfg)
            {
                Ok(typedef) => {
                    info!("Take {}::{}.", crate_name, &item.ident);
                    self.typedefs.try_insert(typedef);
                    return;
                }
                Err(msg) => msg,
            }
        } else {
            format!("Cannot have generics in typedef.")
        };

        let fail2 = match Specialization::load(alias_name.clone(),
                                               generics,
                                               ty,
                                               &item.attrs,
                                               mod_cfg) {
            Ok(spec) => {
                info!("Take {}::{}.", crate_name, &item.ident);
                self.specializations.try_insert(spec);
                return;
            }
            Err(msg) => msg,
        };

        info!("Skip {}::{} - ({} and {}).",
              crate_name,
              &item.ident,
              fail1,
              fail2);
    }
}
