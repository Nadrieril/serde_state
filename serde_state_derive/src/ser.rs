use crate::{
    attrs::parse_field_attrs,
    dummy,
    mode::{attrs_mode, merge_modes, ItemMode},
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    parse_quote, Attribute, Data, DataEnum, DataStruct, DeriveInput, Fields, FieldsNamed,
    FieldsUnnamed, Generics, Type,
};

pub fn expand_derive_serialize(input: &DeriveInput) -> syn::Result<TokenStream> {
    let attrs = ContainerAttributes::from_attrs(&input.attrs)?;
    let impl_block = match &input.data {
        Data::Struct(data) => derive_struct(input, data, &attrs)?,
        Data::Enum(data) => derive_enum(input, data, &attrs)?,
        Data::Union(u) => {
            return Err(syn::Error::new(
                u.union_token.span(),
                "SerializeState does not support unions",
            ));
        }
    };

    Ok(dummy::wrap_in_const(attrs.serde_path.as_ref(), impl_block))
}

fn derive_struct(
    input: &DeriveInput,
    data: &DataStruct,
    attrs: &ContainerAttributes,
) -> syn::Result<TokenStream> {
    let infer_state = attrs.state.is_none();
    let impl_generics_storage = add_state_param(&input.generics, infer_state);
    let (impl_generics_ref, _, _) = impl_generics_storage.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = state_type_tokens(attrs.state.as_ref());
    let field_types = collect_field_types_from_fields(&data.fields, attrs.mode);
    if infer_state {
        add_serialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    } else {
        add_serialize_bounds_from_type_params(
            &mut where_clause,
            &input.generics,
            &state_tokens,
            attrs.mode,
        );
    }
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let ident = &input.ident;

    let body = if attrs.transparent {
        serialize_transparent(&data.fields, attrs.mode)?
    } else {
        serialize_struct_body(ident, &data.fields, attrs.mode)
    };

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics _serde_state::SerializeState<#state_tokens> for #ident #ty_generics #where_clause_tokens {
            fn serialize_state<__S>(
                &self,
                __state: &#state_tokens,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: _serde::Serializer,
            {
                #body
            }
        }
    })
}

fn derive_enum(
    input: &DeriveInput,
    data: &DataEnum,
    attrs: &ContainerAttributes,
) -> syn::Result<TokenStream> {
    let infer_state = attrs.state.is_none();
    let impl_generics_storage = add_state_param(&input.generics, infer_state);
    let (impl_generics_ref, _, _) = impl_generics_storage.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = state_type_tokens(attrs.state.as_ref());
    let field_types = collect_field_types_from_enum(data, attrs.mode);
    if infer_state {
        add_serialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    } else {
        add_serialize_bounds_from_type_params(
            &mut where_clause,
            &input.generics,
            &state_tokens,
            attrs.mode,
        );
    }
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let ident = &input.ident;

    let body = serialize_enum_body(ident, data, attrs.mode);

    Ok(quote! {
        #[automatically_derived]
        impl #impl_generics _serde_state::SerializeState<#state_tokens> for #ident #ty_generics #where_clause_tokens {
            fn serialize_state<__S>(
                &self,
                __state: &#state_tokens,
                __serializer: __S,
            ) -> ::core::result::Result<__S::Ok, __S::Error>
            where
                __S: _serde::Serializer,
            {
                #body
            }
        }
    })
}

fn serialize_transparent(fields: &Fields, mode: ItemMode) -> syn::Result<TokenStream> {
    match fields {
        Fields::Named(named) if named.named.len() == 1 => {
            let field = named.named.first().unwrap();
            let ident = field.ident.as_ref().unwrap();
            Ok(serialize_transparent_call(
                field,
                mode,
                quote!(&self.#ident),
            ))
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let field = unnamed.unnamed.first().unwrap();
            let index = syn::Index::from(0);
            Ok(serialize_transparent_call(
                field,
                mode,
                quote!(&self.#index),
            ))
        }
        other => Err(syn::Error::new(
            other.span(),
            "transparent structs must have exactly one field",
        )),
    }
}

fn serialize_transparent_call(
    field: &syn::Field,
    default_mode: ItemMode,
    value: TokenStream,
) -> TokenStream {
    match merge_modes(default_mode, attrs_mode(&field.attrs)) {
        ItemMode::Stateful => quote! {
            _serde_state::SerializeState::serialize_state(#value, __state, __serializer)
        },
        ItemMode::Stateless => quote! {
            _serde::Serialize::serialize(#value, __serializer)
        },
    }
}

fn serialize_struct_body(ident: &syn::Ident, fields: &Fields, mode: ItemMode) -> TokenStream {
    match fields {
        Fields::Named(named) => serialize_named_fields(ident, named, mode),
        Fields::Unnamed(unnamed) => serialize_unnamed_fields(ident, unnamed, mode),
        Fields::Unit => serialize_unit_struct(ident),
    }
}

fn serialize_named_fields(ident: &syn::Ident, fields: &FieldsNamed, mode: ItemMode) -> TokenStream {
    let type_name = ident.to_string();
    let field_infos: Vec<_> = fields
        .named
        .iter()
        .map(|field| (field, parse_field_attrs(&field.attrs)))
        .collect();
    let len = field_infos.iter().filter(|(_, attrs)| !attrs.skip).count();
    let serialize_fields =
        field_infos
            .iter()
            .filter(|(_, attrs)| !attrs.skip)
            .map(|(field, attrs)| {
                let field_ident = field.ident.as_ref().unwrap();
                let key = attrs.key(field_ident);
                let call = serialize_field_expr(field, mode, quote!(&self.#field_ident));
                quote! {
                    _serde::ser::SerializeStruct::serialize_field(
                        &mut __serde_state,
                        #key,
                        #call,
                    )?;
                }
            });

    quote! {
        let mut __serde_state = _serde::Serializer::serialize_struct(__serializer, #type_name, #len)?;
        #(#serialize_fields)*
        _serde::ser::SerializeStruct::end(__serde_state)
    }
}

fn serialize_unnamed_fields(
    ident: &syn::Ident,
    fields: &FieldsUnnamed,
    mode: ItemMode,
) -> TokenStream {
    match fields.unnamed.len() {
        0 => serialize_unit_struct(ident),
        1 => {
            let index = syn::Index::from(0);
            let call =
                serialize_field_expr(fields.unnamed.first().unwrap(), mode, quote!(&self.#index));
            quote! {
                _serde::Serializer::serialize_newtype_struct(
                    __serializer,
                    stringify!(#ident),
                    #call,
                )
            }
        }
        len => {
            let serialize_fields = fields.unnamed.iter().enumerate().map(|(i, field)| {
                let index = syn::Index::from(i);
                let call = serialize_field_expr(field, mode, quote!(&self.#index));
                quote! {
                    _serde::ser::SerializeTupleStruct::serialize_field(
                        &mut __serde_state,
                        #call,
                    )?;
                }
            });
            quote! {
                let mut __serde_state = _serde::Serializer::serialize_tuple_struct(
                    __serializer,
                    stringify!(#ident),
                    #len,
                )?;
                #(#serialize_fields)*
                _serde::ser::SerializeTupleStruct::end(__serde_state)
            }
        }
    }
}

fn serialize_unit_struct(ident: &syn::Ident) -> TokenStream {
    quote! {
        _serde::Serializer::serialize_unit_struct(__serializer, stringify!(#ident))
    }
}

fn serialize_field_expr(
    field: &syn::Field,
    default_mode: ItemMode,
    value: TokenStream,
) -> TokenStream {
    match merge_modes(default_mode, attrs_mode(&field.attrs)) {
        ItemMode::Stateful => {
            quote!(&_serde_state::__private::wrap_serialize(#value, __state))
        }
        ItemMode::Stateless => quote!(#value),
    }
}

fn serialize_enum_body(ident: &syn::Ident, data: &DataEnum, mode: ItemMode) -> TokenStream {
    let type_name = ident.to_string();
    let variants = data.variants.iter().enumerate().map(|(index, variant)| {
        let variant_mode = merge_modes(mode, attrs_mode(&variant.attrs));
        serialize_enum_variant(variant, index as u32, &type_name, variant_mode)
    });

    quote! {
        match self {
            #(#variants)*
        }
    }
}

fn serialize_enum_variant(
    variant: &syn::Variant,
    index: u32,
    type_name: &str,
    mode: ItemMode,
) -> TokenStream {
    let variant_ident = &variant.ident;
    let variant_name = variant_ident.to_string();
    match &variant.fields {
        Fields::Unit => {
            quote! {
                Self::#variant_ident => {
                    _serde::Serializer::serialize_unit_variant(
                        __serializer,
                        #type_name,
                        #index,
                        #variant_name,
                    )
                }
            }
        }
        Fields::Unnamed(fields) if fields.unnamed.len() == 1 => {
            let binding = format_ident!("__variant_{}_field", index);
            let field = &fields.unnamed.first().unwrap();
            let call = serialize_field_expr(field, mode, quote!(#binding));
            quote! {
                Self::#variant_ident(ref #binding) => {
                    _serde::Serializer::serialize_newtype_variant(
                        __serializer,
                        #type_name,
                        #index,
                        #variant_name,
                        #call,
                    )
                }
            }
        }
        Fields::Unnamed(fields) => {
            let len = fields.unnamed.len();
            let bindings: Vec<_> = (0..len)
                .map(|i| format_ident!("__variant_{}_field{}", index, i))
                .collect();
            let serialize_fields =
                bindings
                    .iter()
                    .zip(fields.unnamed.iter())
                    .map(|(binding, field)| {
                        let call = serialize_field_expr(field, mode, quote!(#binding));
                        quote! {
                            _serde::ser::SerializeTupleVariant::serialize_field(
                                &mut __serde_state,
                                #call,
                            )?;
                        }
                    });
            quote! {
                Self::#variant_ident( #(ref #bindings),* ) => {
                    let mut __serde_state = _serde::Serializer::serialize_tuple_variant(
                        __serializer,
                        #type_name,
                        #index,
                        #variant_name,
                        #len,
                    )?;
                    #(#serialize_fields)*
                    _serde::ser::SerializeTupleVariant::end(__serde_state)
                }
            }
        }
        Fields::Named(fields) => {
            let field_idents: Vec<_> = fields
                .named
                .iter()
                .map(|field| field.ident.as_ref().unwrap())
                .collect();
            let field_infos: Vec<_> = fields
                .named
                .iter()
                .map(|field| (field, parse_field_attrs(&field.attrs)))
                .collect();
            let len = field_infos.iter().filter(|(_, attrs)| !attrs.skip).count();
            let serialize_fields =
                field_infos
                    .iter()
                    .filter(|(_, attrs)| !attrs.skip)
                    .map(|(field, attrs)| {
                        let ident = field.ident.as_ref().unwrap();
                        let name = attrs.key(ident);
                        let call = serialize_field_expr(field, mode, quote!(#ident));
                        quote! {
                            _serde::ser::SerializeStructVariant::serialize_field(
                                &mut __serde_state,
                                #name,
                                #call,
                            )?;
                        }
                    });
            quote! {
                Self::#variant_ident { #(ref #field_idents),* } => {
                    let mut __serde_state = _serde::Serializer::serialize_struct_variant(
                        __serializer,
                        #type_name,
                        #index,
                        #variant_name,
                        #len,
                    )?;
                    #(#serialize_fields)*
                    _serde::ser::SerializeStructVariant::end(__serde_state)
                }
            }
        }
    }
}

struct FieldType<'a> {
    ty: &'a syn::Type,
    mode: ItemMode,
}

impl<'a> FieldType<'a> {
    fn new(ty: &'a syn::Type, mode: ItemMode) -> Self {
        FieldType { ty, mode }
    }
}

fn collect_field_types_from_fields<'a>(
    fields: &'a Fields,
    default_mode: ItemMode,
) -> Vec<FieldType<'a>> {
    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter_map(|field| {
                let attrs = parse_field_attrs(&field.attrs);
                if attrs.skip {
                    return None;
                }
                Some(FieldType::new(
                    &field.ty,
                    merge_modes(default_mode, attrs_mode(&field.attrs)),
                ))
            })
            .collect(),
        Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .filter_map(|field| {
                let attrs = parse_field_attrs(&field.attrs);
                if attrs.skip {
                    return None;
                }
                Some(FieldType::new(
                    &field.ty,
                    merge_modes(default_mode, attrs_mode(&field.attrs)),
                ))
            })
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

fn collect_field_types_from_enum<'a>(
    data: &'a DataEnum,
    default_mode: ItemMode,
) -> Vec<FieldType<'a>> {
    let mut result = Vec::new();
    for variant in &data.variants {
        let variant_mode = merge_modes(default_mode, attrs_mode(&variant.attrs));
        result.extend(collect_field_types_from_fields(
            &variant.fields,
            variant_mode,
        ));
    }
    result
}

fn add_serialize_bounds_from_types(
    where_clause: &mut Option<syn::WhereClause>,
    field_types: &[FieldType<'_>],
    state_ty: &TokenStream,
) {
    if field_types.is_empty() {
        return;
    }

    let clause = where_clause.get_or_insert_with(|| syn::WhereClause {
        where_token: Default::default(),
        predicates: Default::default(),
    });

    for field in field_types {
        let ty = field.ty;
        match field.mode {
            ItemMode::Stateful => clause
                .predicates
                .push(parse_quote!(#ty: _serde_state::SerializeState<#state_ty>)),
            ItemMode::Stateless => clause.predicates.push(parse_quote!(#ty: _serde::Serialize)),
        }
    }
}

fn add_serialize_bounds_from_type_params(
    where_clause: &mut Option<syn::WhereClause>,
    generics: &Generics,
    state_ty: &TokenStream,
    mode: ItemMode,
) {
    let type_params: Vec<_> = generics
        .type_params()
        .map(|param| param.ident.clone())
        .collect();
    if type_params.is_empty() {
        return;
    }

    let clause = where_clause.get_or_insert_with(|| syn::WhereClause {
        where_token: Default::default(),
        predicates: Default::default(),
    });

    for ident in type_params {
        match mode {
            ItemMode::Stateful => clause
                .predicates
                .push(parse_quote!(#ident: _serde_state::SerializeState<#state_ty>)),
            ItemMode::Stateless => clause
                .predicates
                .push(parse_quote!(#ident: _serde::Serialize)),
        }
    }
}

fn state_type_tokens(state: Option<&syn::Type>) -> TokenStream {
    match state {
        Some(ty) => quote!(#ty),
        None => quote!(__State),
    }
}

fn add_state_param(generics: &Generics, infer_state: bool) -> Generics {
    let mut generics = generics.clone();
    if infer_state {
        generics.params.push(parse_quote!(__State: ?Sized));
    }
    generics
}

struct ContainerAttributes {
    transparent: bool,
    serde_path: Option<syn::Path>,
    state: Option<Type>,
    mode: ItemMode,
}

impl ContainerAttributes {
    fn from_attrs(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = ContainerAttributes {
            transparent: false,
            serde_path: None,
            state: None,
            mode: ItemMode::Stateful,
        };

        for attr in attrs {
            let is_serde = attr.path().is_ident("serde");
            let is_serde_state = attr.path().is_ident("serde_state");
            if !(is_serde || is_serde_state) {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("transparent") {
                    result.transparent = true;
                    return Ok(());
                }
                if meta.path.is_ident("crate") {
                    let path = meta.value()?.parse()?;
                    result.serde_path = Some(path);
                    return Ok(());
                }
                if meta.path.is_ident("state") {
                    if !is_serde_state {
                        return Err(
                            meta.error("`state` must be specified with `serde_state(state = ..)`")
                        );
                    }
                    if result.state.is_some() {
                        return Err(meta.error("duplicate `state` attribute"));
                    }
                    let ty = meta.value()?.parse()?;
                    result.state = Some(ty);
                    return Ok(());
                }
                if meta.path.is_ident("stateless") {
                    if !is_serde_state {
                        return Err(meta.error("`stateless` must be specified with `serde_state`"));
                    }
                    result.mode = ItemMode::Stateless;
                    return Ok(());
                }
                if meta.path.is_ident("stateful") {
                    if !is_serde_state {
                        return Err(meta.error("`stateful` must be specified with `serde_state`"));
                    }
                    result.mode = ItemMode::Stateful;
                    return Ok(());
                }
                if is_serde_state {
                    Err(meta.error("unsupported serde_state attribute"))
                } else {
                    Err(meta.error("unsupported serde attribute"))
                }
            })?;
        }

        Ok(result)
    }
}
