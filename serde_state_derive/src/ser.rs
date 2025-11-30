use crate::dummy;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use syn::spanned::Spanned;
use syn::{
    parse_quote, Attribute, Data, DataEnum, DataStruct, DeriveInput, Fields, FieldsNamed,
    FieldsUnnamed, Generics,
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
    let impl_generics_storage = add_state_param(&input.generics);
    let (impl_generics_ref, _, _) = impl_generics_storage.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = quote!(__State);
    let field_types = collect_field_types_from_fields(&data.fields);
    add_serialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let ident = &input.ident;

    let body = if attrs.transparent {
        serialize_transparent(&data.fields)?
    } else {
        serialize_struct_body(ident, &data.fields)
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
    _attrs: &ContainerAttributes,
) -> syn::Result<TokenStream> {
    let impl_generics_storage = add_state_param(&input.generics);
    let (impl_generics_ref, _, _) = impl_generics_storage.split_for_impl();
    let impl_generics = quote!(#impl_generics_ref);
    let (_, ty_generics_ref, _) = input.generics.split_for_impl();
    let ty_generics = quote!(#ty_generics_ref);
    let mut where_clause = input.generics.where_clause.clone();
    let state_tokens = quote!(__State);
    let field_types = collect_field_types_from_enum(data);
    add_serialize_bounds_from_types(&mut where_clause, &field_types, &state_tokens);
    let where_clause_tokens = match &where_clause {
        Some(clause) => quote!(#clause),
        None => TokenStream::new(),
    };
    let ident = &input.ident;

    let body = serialize_enum_body(ident, data);

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

fn serialize_transparent(fields: &Fields) -> syn::Result<TokenStream> {
    match fields {
        Fields::Named(named) if named.named.len() == 1 => {
            let field = named.named.first().unwrap().ident.as_ref().unwrap();
            Ok(quote! {
                _serde_state::SerializeState::serialize_state(&self.#field, __state, __serializer)
            })
        }
        Fields::Unnamed(unnamed) if unnamed.unnamed.len() == 1 => {
            let field = syn::Index::from(0);
            Ok(quote! {
                _serde_state::SerializeState::serialize_state(&self.#field, __state, __serializer)
            })
        }
        other => Err(syn::Error::new(
            other.span(),
            "transparent structs must have exactly one field",
        )),
    }
}

fn serialize_struct_body(ident: &syn::Ident, fields: &Fields) -> TokenStream {
    match fields {
        Fields::Named(named) => serialize_named_fields(ident, named),
        Fields::Unnamed(unnamed) => serialize_unnamed_fields(ident, unnamed),
        Fields::Unit => serialize_unit_struct(ident),
    }
}

fn serialize_named_fields(ident: &syn::Ident, fields: &FieldsNamed) -> TokenStream {
    let type_name = ident.to_string();
    let len = fields.named.len();
    let serialize_fields = fields.named.iter().map(|field| {
        let field_ident = field.ident.as_ref().unwrap();
        let key = field_ident.to_string();
        quote! {
            _serde::ser::SerializeStruct::serialize_field(
                &mut __serde_state,
                #key,
                &_serde_state::__private::wrap_serialize(&self.#field_ident, __state),
            )?;
        }
    });

    quote! {
        let mut __serde_state = _serde::Serializer::serialize_struct(__serializer, #type_name, #len)?;
        #(#serialize_fields)*
        _serde::ser::SerializeStruct::end(__serde_state)
    }
}

fn serialize_unnamed_fields(ident: &syn::Ident, fields: &FieldsUnnamed) -> TokenStream {
    match fields.unnamed.len() {
        0 => serialize_unit_struct(ident),
        1 => {
            let index = syn::Index::from(0);
            quote! {
                _serde::Serializer::serialize_newtype_struct(
                    __serializer,
                    stringify!(#ident),
                    &_serde_state::__private::wrap_serialize(&self.#index, __state),
                )
            }
        }
        len => {
            let serialize_fields = (0..len).map(|i| {
                let index = syn::Index::from(i);
                quote! {
                    _serde::ser::SerializeTupleStruct::serialize_field(
                        &mut __serde_state,
                        &_serde_state::__private::wrap_serialize(&self.#index, __state),
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

fn serialize_enum_body(ident: &syn::Ident, data: &DataEnum) -> TokenStream {
    let type_name = ident.to_string();
    let variants = data
        .variants
        .iter()
        .enumerate()
        .map(|(index, variant)| serialize_enum_variant(variant, index as u32, &type_name));

    quote! {
        match self {
            #(#variants)*
        }
    }
}

fn serialize_enum_variant(variant: &syn::Variant, index: u32, type_name: &str) -> TokenStream {
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
            quote! {
                Self::#variant_ident(ref #binding) => {
                    _serde::Serializer::serialize_newtype_variant(
                        __serializer,
                        #type_name,
                        #index,
                        #variant_name,
                        &_serde_state::__private::wrap_serialize(#binding, __state),
                    )
                }
            }
        }
        Fields::Unnamed(fields) => {
            let len = fields.unnamed.len();
            let bindings: Vec<_> = (0..len)
                .map(|i| format_ident!("__variant_{}_field{}", index, i))
                .collect();
            let serialize_fields = bindings.iter().map(|binding| {
                quote! {
                    _serde::ser::SerializeTupleVariant::serialize_field(
                        &mut __serde_state,
                        &_serde_state::__private::wrap_serialize(#binding, __state),
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
            let field_names: Vec<_> = field_idents.iter().map(|ident| ident.to_string()).collect();
            let len = field_idents.len();
            let serialize_fields =
                field_idents
                    .iter()
                    .zip(field_names.iter())
                    .map(|(ident, name)| {
                        quote! {
                            _serde::ser::SerializeStructVariant::serialize_field(
                                &mut __serde_state,
                                #name,
                                &_serde_state::__private::wrap_serialize(#ident, __state),
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

fn collect_field_types_from_fields<'a>(fields: &'a Fields) -> Vec<&'a syn::Type> {
    match fields {
        Fields::Named(named) => named
            .named
            .iter()
            .filter(|field| !is_recursive_field(field))
            .map(|field| &field.ty)
            .collect(),
        Fields::Unnamed(unnamed) => unnamed
            .unnamed
            .iter()
            .filter(|field| !is_recursive_field(field))
            .map(|field| &field.ty)
            .collect(),
        Fields::Unit => Vec::new(),
    }
}

fn collect_field_types_from_enum<'a>(data: &'a DataEnum) -> Vec<&'a syn::Type> {
    let mut result = Vec::new();
    for variant in &data.variants {
        result.extend(collect_field_types_from_fields(&variant.fields));
    }
    result
}

fn add_serialize_bounds_from_types(
    where_clause: &mut Option<syn::WhereClause>,
    field_types: &[&syn::Type],
    state_ty: &TokenStream,
) {
    if field_types.is_empty() {
        return;
    }

    let clause = where_clause.get_or_insert_with(|| syn::WhereClause {
        where_token: Default::default(),
        predicates: Default::default(),
    });

    for ty in field_types {
        clause
            .predicates
            .push(parse_quote!(#ty: _serde_state::SerializeState<#state_ty>));
    }
}

fn add_state_param(generics: &Generics) -> Generics {
    let mut generics = generics.clone();
    generics.params.push(parse_quote!(__State: ?Sized));
    generics
}

fn is_recursive_field(field: &syn::Field) -> bool {
    field.attrs.iter().any(|attr| {
        if attr.path().is_ident("serde_state") {
            let mut recursive = false;
            let _ = attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("recursive") {
                    recursive = true;
                }
                Ok(())
            });
            return recursive;
        }
        false
    })
}

struct ContainerAttributes {
    transparent: bool,
    serde_path: Option<syn::Path>,
}

impl ContainerAttributes {
    fn from_attrs(attrs: &[Attribute]) -> syn::Result<Self> {
        let mut result = ContainerAttributes {
            transparent: false,
            serde_path: None,
        };

        for attr in attrs {
            if !(attr.path().is_ident("serde") || attr.path().is_ident("serde_state")) {
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
                    return Err(meta.error(
                        "`serde_state(state = ..)` is no longer supported; the derive now infers the state",
                    ));
                }
                Err(meta.error("unsupported serde attribute"))
            })?;
        }

        Ok(result)
    }
}
